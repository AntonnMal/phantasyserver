use super::HResult;
use crate::{create_attr_files, mutex::MutexGuard, user::User, Action};
use indicatif::HumanBytes;
use memory_stats::memory_stats;
use pso2packetlib::protocol::{chat::ChatArea, flag::FlagType, items::ItemId, Packet};

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
            "!reload_map" => {
                let Some(map) = user.get_current_map() else {
                    unreachable!("User should be in state >= `InGame`")
                };
                drop(user);
                map.lock().await.reload_objs().await?;
            }
            "!start_con" => {
                let name = args.next();
                if name.is_none() {
                    user.send_system_msg("No concert name provided")?;
                    return Ok(Action::Nothing);
                }
                let name = name.unwrap();
                let packet = Packet::SetTag(pso2packetlib::protocol::objects::SetTagPacket {
                    receiver: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: pso2packetlib::protocol::EntityType::Player,
                        ..Default::default()
                    },
                    target: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: pso2packetlib::protocol::EntityType::Object,
                        ..Default::default()
                    },
                    object3: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: pso2packetlib::protocol::EntityType::Object,
                        ..Default::default()
                    },
                    attribute: format!("Start({name})").into(),
                    ..Default::default()
                });
                user.send_packet(&packet)?;
            }
            "!send_con" => {
                let name = args.next();
                if name.is_none() {
                    user.send_system_msg("No action provided")?;
                    return Ok(Action::Nothing);
                }
                let name = name.unwrap();
                let packet = Packet::SetTag(pso2packetlib::protocol::objects::SetTagPacket {
                    receiver: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: pso2packetlib::protocol::EntityType::Player,
                        ..Default::default()
                    },
                    target: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: pso2packetlib::protocol::EntityType::Object,
                        ..Default::default()
                    },
                    object3: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: pso2packetlib::protocol::EntityType::Player,
                        ..Default::default()
                    },
                    attribute: name.into(),
                    ..Default::default()
                });
                user.send_packet(&packet)?;
            }
            "!get_pos" => {
                let pos = user.position;
                let pos: pso2packetlib::protocol::models::EulerPosition = pos.into();
                user.send_system_msg(&format!("{pos:?}"))?;
            }
            "!get_close_obj" => {
                let dist = args.next().and_then(|n| n.parse().ok()).unwrap_or(1.0);
                let Some(map) = user.get_current_map() else {
                    unreachable!("User should be in state >= `InGame`")
                };
                let mapid = user.mapid;
                let lock = map.lock().await;
                let objs = lock.get_close_objects(mapid, |p| user.position.dist_2d(p) < dist);
                let user_pos = user.position;
                for obj in objs {
                    user.send_system_msg(&format!(
                        "Id: {}, Name: {}, Dist: {}",
                        obj.object.id,
                        obj.name,
                        user_pos.dist_2d(&obj.position)
                    ))?;
                }
            }
            "!reload_items" => {
                let (pc, vita) = {
                    tokio::task::spawn_blocking(create_attr_files)
                        .await
                        .unwrap()?
                };
                let mut attrs = user.blockdata.item_attrs.write().await;
                attrs.pc_attrs = pc;
                attrs.vita_attrs = vita;
                drop(attrs);
                user.send_item_attrs().await?;
                user.send_system_msg("Done!")?;
            }
            "!set_acc_flag" => set_flag_parse(&mut user, FlagType::Account, &mut args)?,
            "!set_char_flag" => set_flag_parse(&mut user, FlagType::Character, &mut args)?,
            "!add_item" => {
                let Some(item_type) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No item type provided")?;
                    return Ok(Action::Nothing);
                };
                let Some(id) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No id provided")?;
                    return Ok(Action::Nothing);
                };
                let Some(subid) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No subid provided")?;
                    return Ok(Action::Nothing);
                };
                let item_id = ItemId {
                    id,
                    subid,
                    item_type,
                    ..Default::default()
                };
                let user: &mut User = &mut user;
                let character = user.character.as_mut().unwrap();
                let packet = character
                    .inventory
                    .add_default_item(&mut user.uuid, item_id);
                user.send_packet(&packet)?;
            }
            _ => user.send_system_msg("Unknown command")?,
        }
        return Ok(Action::Nothing);
    }
    if ChatArea::Map == data.area {
        let id = user.player_id;
        let map = user.get_current_map();
        drop(user);
        if let Some(map) = map {
            map.lock().await.send_message(packet, id).await;
        }
    } else if ChatArea::Party == data.area {
        let id = user.player_id;
        let party = user.get_current_party();
        drop(user);
        if let Some(party) = party {
            party.read().await.send_message(packet, id).await;
        }
    }
    Ok(Action::Nothing)
}

fn set_flag_parse<'a>(
    user: &mut User,
    ftype: FlagType,
    args: &mut impl Iterator<Item = &'a str>,
) -> Result<(), crate::Error> {
    let range = match args.next() {
        Some(r) => r,
        None => {
            user.send_system_msg("No range provided")?;
            return Ok(());
        }
    };
    let val = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
    if range.contains('-') {
        let mut split = range.split('-');
        let lower = split.next().and_then(|r| r.parse().ok());
        let upper = split.next().and_then(|r| r.parse().ok());
        let (Some(lower), Some(upper)) = (lower, upper) else {
            user.send_system_msg("Invalid range")?;
            return Ok(());
        };
        if lower > upper {
            user.send_system_msg("Invalid range")?;
            return Ok(());
        }
        for i in lower..=upper {
            set_flag(user, ftype, i, val)?;
        }
    } else {
        let id = match range.parse() {
            Ok(i) => i,
            Err(_) => {
                user.send_system_msg("Invalid id")?;
                return Ok(());
            }
        };
        set_flag(user, ftype, id, val)?;
    }
    Ok(())
}

fn set_flag(user: &mut User, ftype: FlagType, id: usize, val: u8) -> Result<(), crate::Error> {
    let character = user.character.as_mut().unwrap();
    match ftype {
        FlagType::Account => user.accountflags.set(id, val),
        FlagType::Character => character.flags.set(id, val),
    };
    user.send_packet(&Packet::ServerSetFlag(
        pso2packetlib::protocol::flag::ServerSetFlagPacket {
            flag_type: ftype,
            id: id as u32,
            value: val as u32,
            ..Default::default()
        },
    ))?;

    Ok(())
}
