use super::HResult;
use crate::{async_lock, async_write, create_attr_files, user::User, Action};
use indicatif::HumanBytes;
use memory_stats::memory_stats;
use parking_lot::MutexGuard;
use pso2packetlib::protocol::{chat::ChatArea, Packet};

pub async fn send_chat(mut user: MutexGuard<'_, User>, packet: Packet) -> HResult {
    let Packet::ChatMessage(ref data) = packet else {
        unreachable!()
    };
    if data.message.starts_with('!') {
        let mut args = data.message.split(' ');
        let cmd = args.next().expect("Should always contain some data");
        match cmd {
            "!mem" => {
                let mem_data_msg = if let Some(mem) = memory_stats() {
                    format!(
                        "Physical memory: {}\nVirtual memory: {}",
                        HumanBytes(mem.physical_mem as u64),
                        HumanBytes(mem.virtual_mem as u64),
                    )
                } else {
                    "Couldn't gather memory info".into()
                };
                user.send_system_msg(&mem_data_msg)?;
            }
            "!reload_map_lua" => {
                if let Some(ref map) = user.map {
                    async_lock(map).await.reload_lua()?;
                }
            }
            "!map_gc" => {
                if let Some(ref map) = user.map {
                    async_lock(map).await.lua_gc_collect()?;
                }
            }
            "!reload_map" => {
                if let Some(ref map) = user.map {
                    let map = map.clone();
                    //SAFETY: this ref will live as long as the closure because we await it
                    let lock: &'static mut MutexGuard<User> =
                        unsafe { std::mem::transmute(&mut user) };
                    tokio::task::spawn_blocking(move || {
                        MutexGuard::unlocked(lock, || map.lock().reload_objs())
                    })
                    .await
                    .unwrap()?
                }
            }
            "!reload_items" => {
                let mul_progress = indicatif::MultiProgress::new();
                let (pc, vita) = {
                    //SAFETY: this ref will live as long as the closure because we await it
                    let lock: &'static mut MutexGuard<User> =
                        unsafe { std::mem::transmute(&mut user) };
                    tokio::task::spawn_blocking(move || {
                        MutexGuard::unlocked(lock, || create_attr_files(&mul_progress))
                    })
                    .await
                    .unwrap()?
                };
                let mut attrs = async_write(&user.blockdata.item_attrs).await;
                attrs.pc_attrs = pc;
                attrs.vita_attrs = vita;
                drop(attrs);
                user.send_item_attrs()?;
                user.send_system_msg("Done!")?;
            }
            _ => user.send_system_msg("Unknown command")?,
        }
        return Ok(Action::Nothing);
    }
    if let ChatArea::Map = data.area {
        let id = user.player_id;
        let map = user.map.clone();
        drop(user);
        if let Some(map) = map {
            tokio::task::spawn_blocking(move || map.lock().send_message(packet, id))
                .await
                .unwrap();
        }
    }
    Ok(Action::Nothing)
}
