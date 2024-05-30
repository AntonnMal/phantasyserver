use data_structs::{
    inventory::ItemParameters,
    map::MapData,
    quest::QuestData,
    stats::{ClassStatsStored, PlayerStats, RaceModifierStored},
    SerDeFile as _,
};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

fn main() {
    let mut args = env::args();
    args.next();
    let filename = args.next().expect("Input filename");
    let data_type = args.next().expect("Input data type");
    let mut filename = PathBuf::from(filename);
    match data_type.as_str() {
        "map" => {
            if filename.extension().unwrap() == "json" {
                let data = MapData::load_from_json_file(&filename).unwrap();
                filename.set_extension("mp");
                data.save_to_mp_file(&filename).unwrap();
            } else if filename.extension().unwrap() == "mp" {
                let data = MapData::load_from_mp_file(&filename).unwrap();
                filename.set_extension("json");
                data.save_to_json_file(&filename).unwrap();
            }
        }
        "item_name" => {
            if filename.extension().unwrap() == "json" {
                let data = ItemParameters::load_from_json_file(&filename).unwrap();
                filename.set_extension("mp");
                data.save_to_mp_file(&filename).unwrap();
            } else if filename.extension().unwrap() == "mp" {
                let data = ItemParameters::load_from_mp_file(&filename).unwrap();
                filename.set_extension("json");
                data.save_to_json_file(&filename).unwrap();
            }
        }
        "quest" => {
            if filename.extension().unwrap() == "json" {
                let data = QuestData::load_from_json_file(&filename).unwrap();
                filename.set_extension("mp");
                data.save_to_mp_file(&filename).unwrap();
            } else if filename.extension().unwrap() == "mp" {
                let data = QuestData::load_from_mp_file(&filename).unwrap();
                filename.set_extension("json");
                data.save_to_json_file(&filename).unwrap();
            }
        }
        "class_level" => {
            if filename.extension().unwrap() == "json" {
                let data =
                    data_structs::stats::ClassStatsStored::load_from_json_file(&filename).unwrap();
                println!("{data:?}");
            }
        }
        "data_dir" => {
            // parse maps
            let mut map_dir = filename.to_path_buf();
            map_dir.push("maps");
            find_data_dir(&map_dir, parse_map).unwrap();

            // parse quests
            let mut quest_dir = filename.to_path_buf();
            quest_dir.push("quests");
            find_data_dir(&quest_dir, parse_quest).unwrap();

            // parse item names
            let mut names_file = filename.to_path_buf();
            names_file.push("item_names.json");
            if names_file.is_file() {
                let data = ItemParameters::load_from_json_file(&names_file).unwrap();
                names_file.set_extension("mp");
                data.save_to_mp_file(&names_file).unwrap();
            }

            // parse player stats
            let mut player_stats_dir = filename.to_path_buf();
            player_stats_dir.push("class_stats");
            parse_player_stats(&player_stats_dir).unwrap();
        }
        _ => panic!("Invalid type"),
    }
}

fn parse_map(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut data_file = path.to_path_buf();
    data_file.push("data.json");
    let mut data = MapData::load_from_json_file(&data_file)?;

    collect_map_data(path, &mut data)?;

    data_file.pop();
    data_file.set_extension("mp");
    data.save_to_mp_file(data_file)?;
    Ok(())
}

fn collect_map_data(map_path: &Path, map: &mut MapData) -> Result<(), Box<dyn Error>> {
    // load lua files
    let mut lua_dir = map_path.to_path_buf();
    lua_dir.push("luas");
    if lua_dir.exists() {
        traverse_data_dir(lua_dir, &mut |p| {
            let lua = fs::read_to_string(p)?;
            let filename = p.file_stem().unwrap().to_string_lossy().to_string();
            map.luas.insert(filename, lua);
            Ok(())
        })?;
    }

    // load object files
    let mut object_dir = map_path.to_path_buf();
    object_dir.push("objects");
    if object_dir.exists() {
        traverse_data_dir(object_dir, &mut |p| {
            let mut objects = Vec::load_from_json_file(p)?;
            map.objects.append(&mut objects);
            Ok(())
        })?;
    }

    // load transporters files
    let mut transporter_dir = map_path.to_path_buf();
    transporter_dir.push("transporters");
    if transporter_dir.exists() {
        traverse_data_dir(transporter_dir, &mut |p| {
            let mut objects = Vec::load_from_json_file(p)?;
            map.transporters.append(&mut objects);
            Ok(())
        })?;
    }

    // load event files
    let mut event_dir = map_path.to_path_buf();
    event_dir.push("events");
    if event_dir.exists() {
        traverse_data_dir(event_dir, &mut |p| {
            let mut objects = Vec::load_from_json_file(p)?;
            map.events.append(&mut objects);
            Ok(())
        })?;
    }

    // load npc files
    let mut npc_dir = map_path.to_path_buf();
    npc_dir.push("npcs");
    if npc_dir.exists() {
        traverse_data_dir(npc_dir, &mut |p| {
            let mut objects = Vec::load_from_json_file(p)?;
            map.npcs.append(&mut objects);
            Ok(())
        })?;
    }
    Ok(())
}

fn parse_quest(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut data_file = path.to_path_buf();
    data_file.push("data.json");
    let mut data = QuestData::load_from_json_file(&data_file)?;

    // load map
    let mut map_dir = path.to_path_buf();
    map_dir.push("map");
    if map_dir.exists() {
        map_dir.push("map.json");
        data.map = MapData::load_from_json_file(&map_dir)?;
        map_dir.pop();
        collect_map_data(&map_dir, &mut data.map)?;
    }
    // load npc files
    let mut enemy_dir = path.to_path_buf();
    enemy_dir.push("enemies");
    if enemy_dir.exists() {
        traverse_data_dir(enemy_dir, &mut |p| {
            let mut objects = Vec::load_from_json_file(p)?;
            data.enemies.append(&mut objects);
            Ok(())
        })?;
    }

    data_file.pop();
    data_file.set_extension("mp");
    data.save_to_mp_file(data_file)?;
    Ok(())
}

fn parse_player_stats(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut data = PlayerStats::default();

    // load level modifiers
    let mut level_mod_path = path.to_path_buf();
    level_mod_path.push("level_modifiers.json");
    if level_mod_path.is_file() {
        let mod_data = RaceModifierStored::load_from_json_file(&level_mod_path)?;
        data.modifiers.push(mod_data.human_male);
        data.modifiers.push(mod_data.human_female);
        data.modifiers.push(mod_data.newman_male);
        data.modifiers.push(mod_data.newman_female);
        data.modifiers.push(mod_data.cast_male);
        data.modifiers.push(mod_data.cast_female);
        data.modifiers.push(mod_data.deuman_male);
        data.modifiers.push(mod_data.deuman_female);
    }

    // load class stats
    let mut max_class = 0;
    traverse_data_dir(path, &mut |p| {
        if path.file_name().unwrap().to_string_lossy() == "level_modifiers.json" {
            return Ok(());
        }
        let stats = ClassStatsStored::load_from_json_file(p)?;
        let class_int = stats.class as usize;
        if class_int >= max_class {
            max_class = class_int;
            data.stats.resize(class_int + 1, Default::default());
        }
        data.stats[class_int] = stats.stats;
        Ok(())
    })?;

    let mut out_path = path.to_owned();
    out_path.set_file_name("player_stats.mp");
    data.save_to_mp_file(out_path)?;
    Ok(())
}

fn find_data_dir<P, F>(path: P, callback: F) -> Result<(), Box<dyn Error>>
where
    P: AsRef<Path>,
    F: Fn(&Path) -> Result<(), Box<dyn Error>> + Copy,
{
    // find data.json
    if fs::read_dir(&path)?.any(|p| p.unwrap().file_name().to_str().unwrap() == "data.json") {
        return callback(path.as_ref());
    }

    let dir = fs::read_dir(path)?;
    for entry in dir {
        let entry = entry?.path();
        if entry.is_dir() {
            find_data_dir(entry, callback)?;
        }
    }
    Ok(())
}

fn traverse_data_dir<P, F>(path: P, callback: &mut F) -> Result<(), Box<dyn Error>>
where
    P: AsRef<Path>,
    F: FnMut(&Path) -> Result<(), Box<dyn Error>>,
{
    for entry in fs::read_dir(path)? {
        let entry = entry?.path();
        if entry.is_dir() {
            traverse_data_dir(entry, callback)?;
        } else if entry.is_file() {
            callback(&entry)?;
        }
    }
    Ok(())
}
