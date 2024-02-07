use crate::{
    map,
    mutex::{Mutex, RwLock},
    party, sql,
    user::{User, UserState},
    Action, BlockData, BlockInfo, Error,
};
use data_structs::inventory::ItemParameters;
use pso2packetlib::PrivateKey;
use std::{
    io,
    net::TcpStream,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::mpsc};

pub async fn init_block(
    blocks: Arc<RwLock<Vec<BlockInfo>>>,
    this_block: BlockInfo,
    sql: Arc<sql::Sql>,
    item_attrs: Arc<RwLock<ItemParameters>>,
    key: PrivateKey,
) -> Result<(), Error> {
    let listener = TcpListener::bind(("0.0.0.0", this_block.port)).await?;

    let latest_mapid = AtomicU32::new(0);

    let lobby = Arc::new(Mutex::new(map::Map::new(
        this_block.lobby_map,
        &latest_mapid,
    )?));

    let block_data = Arc::new(BlockData {
        sql,
        blocks,
        item_attrs,
        block_id: this_block.id,
        block_name: this_block.name,
        lobby,
        key,
        latest_mapid,
        latest_partyid: AtomicU32::new(0),
        quests: this_block.quests,
    });

    let mut clients = vec![];
    let mut conn_id = 0usize;
    let (send, mut recv) = mpsc::channel(10);

    loop {
        tokio::select! {
            // we opt out of random selection because the listener is rarely accepting
            biased;
            result = listener.accept() => {
                let (stream, _) = result?;
                new_conn_handler(
                    stream.into_std()?,
                    &block_data,
                    &mut clients,
                    send.clone(),
                    this_block.id,
                    &mut conn_id,
                )
                .await?;
            }
            Some((id, action)) = recv.recv() => {
                match run_action(&mut clients, id, action, &block_data).await {
                    Ok(_) => {}
                    Err(e) => log::warn!("Client error: {e}"),
                };
            }
        };
    }
}

async fn new_conn_handler(
    s: TcpStream,
    block_data: &Arc<BlockData>,
    clients: &mut Vec<(usize, Arc<Mutex<User>>)>,
    send: mpsc::Sender<(usize, Action)>,
    block_id: u32,
    conn_id_ref: &mut usize,
) -> Result<(), Error> {
    log::info!("Client connected");

    let mut lock = block_data.blocks.write().await;
    if let Some(block) = lock.iter_mut().find(|x| x.id == block_id) {
        if block.players >= block.max_players {
            return Ok(());
        }
        block.players += 1;
    }
    drop(lock);

    let client = Arc::new(Mutex::new(User::new(s, block_data.clone())?));
    clients.push((*conn_id_ref, client.clone()));
    let conn_id = *conn_id_ref;
    tokio::spawn(async move {
        loop {
            match User::tick(client.lock().await).await {
                Ok(Action::Nothing) => {}
                Ok(Action::Disconnect) => {
                    send.send((conn_id, Action::Disconnect)).await.unwrap();
                    return;
                }
                Ok(a) => {
                    send.send((conn_id, a)).await.unwrap();
                }
                Err(Error::IOError(e)) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(Error::IOError(x)) if x.kind() == io::ErrorKind::ConnectionAborted => {
                    send.send((conn_id, Action::Disconnect)).await.unwrap();
                    return;
                }
                Err(e) => {
                    let error_msg = format!("Client error: {e}");
                    let _ = client.lock().await.send_error(&error_msg);
                    log::warn!("{}", error_msg);
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });

    *conn_id_ref += 1;
    Ok(())
}

async fn run_action(
    clients: &mut Vec<(usize, Arc<Mutex<User>>)>,
    conn_id: usize,
    action: Action,
    block_data: &Arc<BlockData>,
) -> Result<(), Error> {
    let Some((pos, _)) = clients
        .iter()
        .enumerate()
        .find(|(_, (c_conn_id, _))| *c_conn_id == conn_id)
    else {
        return Ok(());
    };
    match action {
        Action::Nothing => {}
        Action::Disconnect => {
            log::info!("Client disconnected");
            clients.remove(pos);

            let mut lock = block_data.blocks.write().await;
            if let Some(block) = lock.iter_mut().find(|x| x.id == block_data.block_id) {
                block.players -= 1;
            }
        }
        Action::InitialLoad => {
            let (_, user) = &clients[pos];
            let mut user_lock = user.lock().await;
            user_lock.set_map(block_data.lobby.clone());
            drop(user_lock);
            let party_id = block_data.latest_partyid.fetch_add(1, Ordering::Relaxed);
            party::Party::init_player(user.clone(), party_id).await?;
            block_data
                .lobby
                .lock()
                .await
                .init_add_player(user.clone())
                .await?;
            let mut user_lock = user.lock().await;
            user_lock.state = UserState::InGame;
        }
        Action::SendPartyInvite(invitee_id) => {
            let (_, inviter) = &clients[pos];

            let mut invitee = None;
            for (_, client) in &*clients {
                if client.lock().await.get_user_id() == invitee_id {
                    invitee = Some(client.clone());
                    break;
                }
            }

            if let Some(invitee) = invitee {
                party::Party::send_invite(inviter.clone(), invitee).await?;
            }
        }
    }
    Ok(())
}
