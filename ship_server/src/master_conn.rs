use crate::Error;
use data_structs::master_ship::{
    MasterShipAction as MAS, MasterShipComm, RegisterShipResult, ShipConnection, ShipInfo,
    ShipLoginResult,
};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::atomic::AtomicU32,
};
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Serialize, Deserialize, Clone, Debug)]
struct HostKey {
    ip: Ipv4Addr,
    key: Vec<u8>,
}

struct MasterConnectionImpl {
    id: u32,
    conn: ShipConnection,
    receive_ch: Receiver<(MAS, Sender<MAS>)>,
}

pub struct MasterConnection {
    send_ch: Sender<(MAS, Sender<MAS>)>,
    local_addr: Ipv4Addr,
    ship_id: AtomicU32,
}

impl MasterConnection {
    pub async fn new(ip: SocketAddr, psk: &[u8]) -> Result<Self, Error> {
        let socket = tokio::net::TcpStream::connect(ip).await?;
        let IpAddr::V4(local_addr) = socket.local_addr()?.ip() else {
            unimplemented!()
        };
        let mut hostkeys: Vec<HostKey> =
            rmp_serde::from_slice(&tokio::fs::read("hostkeys.mp").await.unwrap_or(vec![]))
                .unwrap_or(Default::default());
        let conn = ShipConnection::new_client(socket, |ip, key| {
            if let Some(host) = hostkeys.iter().find(|d| d.ip == ip) {
                return host.key == key;
            }
            let key = key.to_owned();
            hostkeys.push(HostKey { ip, key });
            true
        })
        .await?;
        tokio::fs::write("hostkeys.mp", rmp_serde::to_vec(&hostkeys)?).await?;
        let (send, recv) = tokio::sync::mpsc::channel(10);
        let master_conn = Self {
            send_ch: send,
            local_addr,
            ship_id: 0.into(),
        };

        let master_conn_impl = MasterConnectionImpl {
            id: 1,
            conn,
            receive_ch: recv,
        };
        tokio::spawn(async move { master_conn_impl.run_loop().await });

        let response = master_conn
            .run_action(MAS::ShipLogin { psk: psk.to_vec() })
            .await?;
        match response {
            MAS::ShipLoginResult(ShipLoginResult::Ok) => Ok(master_conn),
            MAS::ShipLoginResult(ShipLoginResult::UnknownShip) => Err(Error::MSInvalidPSK),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn run_action(&self, action: MAS) -> Result<MAS, Error> {
        log::trace!("Request to master ship: {action:?}");
        let (send, mut recv) = tokio::sync::mpsc::channel(1);
        self.send_ch
            .send((action, send))
            .await
            .expect("Channel shouldn't be closed");
        match recv.recv().await {
            Some(d) => Ok(d),
            None => Err(Error::MSNoResponse),
        }
    }
    pub async fn register_ship(&self, mut info: ShipInfo) -> Result<RegisterShipResult, Error> {
        self.ship_id
            .swap(info.id, std::sync::atomic::Ordering::Relaxed);
        info.ip = self.local_addr;
        match self.run_action(MAS::RegisterShip(info)).await? {
            MAS::RegisterShipResult(x) => Ok(x),
            MAS::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
}

impl MasterConnectionImpl {
    async fn run_loop(mut self) {
        let mut channels: Vec<(u32, Sender<MAS>)> = vec![];
        loop {
            tokio::select! {
                result = self.conn.read() => {
                    let result = match result {
                        Ok(r) => r,
                        Err(e) => {
                            log::error!("Failed to receive data from a master server: {e}");
                            return
                        }
                    };
                    let Some((pos, _)) = channels.iter().enumerate().find(|(_, (id,_))| *id == result.id) else {
                        log::error!("Master server sent unhandled response: {result:?}");
                        return;
                    };
                    log::trace!("Master ship sent: {result:?}");
                    let (_, ch) = channels.swap_remove(pos);
                    let _ = ch.send(result.action).await;
                },
                Some((action, chan)) = self.receive_ch.recv() => {
                    let id = self.id;
                    self.id += 1;
                    match self.conn.write(MasterShipComm { id, action }).await {
                        Ok(_) => channels.push((id, chan)),
                        Err(e) => log::error!("Failed to send a request to a master server: {e}"),
                    }
                },
            }
        }
    }
}

impl Drop for MasterConnection {
    fn drop(&mut self) {
        let ship_id = self.ship_id.load(std::sync::atomic::Ordering::Relaxed);
        if ship_id != 0 {
            let (send, _) = tokio::sync::mpsc::channel(1);
            self.send_ch
                .blocking_send((MAS::UnregisterShip(ship_id), send))
                .expect("Channel shouldn't be closed");
        }
    }
}
