use crate::{mutex::Mutex, Error};
use data_structs::master_ship::{
    MasterShipAction as MAS, MasterShipComm, RegisterShipResult, ShipConnection, ShipInfo,
    ShipLoginResult,
};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

#[derive(Serialize, Deserialize, Clone, Debug)]
struct HostKey {
    ip: Ipv4Addr,
    key: Vec<u8>,
}

pub struct MasterConnection {
    id: u32,
    conn: ShipConnection,
    actions: Vec<(u32, MAS)>,
    local_addr: Ipv4Addr,
    ship_id: u32,
}

impl MasterConnection {
    pub async fn new(ip: SocketAddr, psk: &[u8]) -> Result<Mutex<Self>, Error> {
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
        let conn = Mutex::new(Self {
            id: 0,
            conn,
            actions: vec![],
            local_addr,
            ship_id: 0,
        });

        let response = Self::run_action(&conn, MAS::ShipLogin { psk: psk.to_vec() }).await?;
        match response {
            MAS::ShipLoginResult(ShipLoginResult::Ok) => Ok(conn),
            MAS::ShipLoginResult(ShipLoginResult::UnknownShip) => Err(Error::MSInvalidPSK),
            _ => Err(Error::MSUnexpected),
        }
    }
    pub async fn run_action(this: &Mutex<Self>, action: MAS) -> Result<MAS, Error> {
        log::trace!("Request to master ship: {action:?}");
        let call_id = {
            let mut lock = this.lock().await;
            let id = lock.id;
            lock.id += 1;
            lock.conn.write(MasterShipComm { id, action }).await?;
            id
        };
        loop {
            let mut lock = this.lock().await;
            if let Some((pos, _)) = lock
                .actions
                .iter()
                .enumerate()
                .find(|(_, (id, _))| *id == call_id)
            {
                return Ok(lock.actions.swap_remove(pos).1);
            }
            if let Ok(r) = tokio::time::timeout(Duration::from_millis(10), lock.conn.read()).await {
                let r = r?;
                log::trace!("Master ship sent: {:?}", r.action);
                lock.actions.push((r.id, r.action));
            }

            drop(lock);
            tokio::task::yield_now().await;
        }
    }
    pub async fn register_ship(
        this: &Mutex<Self>,
        mut info: ShipInfo,
    ) -> Result<RegisterShipResult, Error> {
        {
            let mut lock = this.lock().await;
            lock.ship_id = info.id;
            info.ip = lock.local_addr;
        }
        match Self::run_action(this, MAS::RegisterShip(info)).await? {
            MAS::RegisterShipResult(x) => Ok(x),
            MAS::Error(e) => Err(Error::MSError(e)),
            _ => Err(Error::MSUnexpected),
        }
    }
}

impl Drop for MasterConnection {
    fn drop(&mut self) {
        if self.ship_id != 0 {
            let _ = self.conn.write_blocking(MasterShipComm {
                id: self.ship_id,
                action: MAS::UnregisterShip(self.ship_id),
            });
        }
    }
}
