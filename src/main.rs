#![no_main]
#![no_std]

extern crate alloc;

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use log::{error, info, warn};
use uefi::prelude::*;
use uefi::proto::device_path::build::{media, DevicePathBuilder};
use uefi::proto::device_path::DevicePath;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::BootPolicy;
use uefi::runtime::{VariableAttributes, VariableVendor};
use uefi::{boot, guid, runtime, CString16};

const CONFIG_PATH: &str = "\\decider\\decider.conf";
const CHOICE_PATH: &str = "\\decider.choice";
const DEFAULT_ENTRIES_PATH: &str = "\\loader\\entries";
const ONESHOT_VAR_NAME: &str = "LoaderEntryOneShot";
const SYSTEMD_BOOT_LOADER_GUID: uefi::Guid = guid!("4a67b082-0a4c-41cf-b6c7-440b29bb8c4f");
const SYSTEMD_BOOT_LOADER_VENDOR: VariableVendor = VariableVendor(SYSTEMD_BOOT_LOADER_GUID);

#[derive(Debug)]
struct Config {
    chainload_path: String,
    entries_path: String,
}

#[derive(Debug)]
enum Choice {
    Entry(String),
    NixosCurrent,
}

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    let image_handle = boot::image_handle();

    match run(image_handle) {
        Ok(()) => Status::SUCCESS,
        Err(status) => {
            error!("decider failed: {:?}", status);
            status
        }
    }
}

fn run(image_handle: Handle) -> Result<(), Status> {
    let config = load_config(image_handle)?;
    info!("loaded config from {}", CONFIG_PATH);
    info!("scanning filesystems...");

    let resolved_entry = match find_choice(image_handle) {
        Ok(Choice::Entry(entry_id)) => {
            info!("parsed entry id: {}", entry_id);
            Some(entry_id)
        }
        Ok(Choice::NixosCurrent) => match find_latest_nixos_entry(image_handle, &config.entries_path) {
            Ok(entry_id) => {
                info!("resolved nixos-current to entry id: {}", entry_id);
                Some(entry_id)
            }
            Err(err) => {
                warn!("failed to resolve nixos-current ({:?}); skipping variable set", err);
                None
            }
        }
        Err(err) => {
            warn!("choice lookup failed ({:?}); skipping variable set", err);
            None
        }
    };

    if let Some(entry_id) = resolved_entry {
        match set_oneshot_variable(&entry_id) {
            Ok(()) => info!("set {} OK", ONESHOT_VAR_NAME),
            Err(err) => warn!("failed to set {} ({:?})", ONESHOT_VAR_NAME, err),
        }
    }

    info!("chainloading {}", config.chainload_path);
    chainload_next(image_handle, &config.chainload_path)?;
    Ok(())
}

fn load_config(image_handle: Handle) -> Result<Config, Status> {
    let mut esp_fs = boot::get_image_file_system(image_handle).map_err(|e| e.status())?;
    let text = read_text_file(&mut esp_fs, CONFIG_PATH)?;
    parse_config(&text)
}

fn find_choice(image_handle: Handle) -> Result<Choice, Status> {
    let source_device = boot::open_protocol_exclusive::<LoadedImage>(image_handle)
        .ok()
        .and_then(|img| img.device());

    let handles = boot::find_handles::<SimpleFileSystem>().map_err(|e| e.status())?;

    for (idx, handle) in handles.into_iter().enumerate() {
        if source_device == Some(handle) {
            continue;
        }

        let mut fs = match boot::open_protocol_exclusive::<SimpleFileSystem>(handle) {
            Ok(fs) => fs,
            Err(err) => {
                warn!("skipping fs #{} (open failed: {:?})", idx, err.status());
                continue;
            }
        };

        let marker_text = match read_text_file(&mut fs, CHOICE_PATH) {
            Ok(text) => text,
            Err(Status::NOT_FOUND) => continue,
            Err(err) => {
                warn!("skipping fs #{} (read failed: {:?})", idx, err);
                continue;
            }
        };

        info!("found choice file on fs #{}", idx);
        return parse_choice(&marker_text);
    }

    Err(Status::NOT_FOUND)
}

fn find_latest_nixos_entry(image_handle: Handle, entries_path: &str) -> Result<String, Status> {
    let mut fs = boot::get_image_file_system(image_handle).map_err(|e| e.status())?;
    let path = to_cstring16(entries_path)?;

    let mut root = fs.open_volume().map_err(|e| e.status())?;
    let handle = root
        .open(path.as_ref(), FileMode::Read, FileAttribute::empty())
        .map_err(|e| e.status())?;

    let mut dir = match handle.into_type().map_err(|e| e.status())? {
        FileType::Dir(dir) => dir,
        _ => return Err(Status::UNSUPPORTED),
    };

    let mut best: Option<(u64, String)> = None;
    loop {
        let Some(info) = dir.read_entry_boxed().map_err(|e| e.status())? else {
            break;
        };

        let name = format!("{}", info.file_name());
        if let Some((generation, entry_id)) = parse_nixos_generation_name(&name) {
            let is_newer = match best {
                Some((best_generation, _)) => generation > best_generation,
                None => true,
            };
            if is_newer {
                best = Some((generation, entry_id));
            }
        }
    }

    best.map(|(_, id)| id).ok_or(Status::NOT_FOUND)
}

fn set_oneshot_variable(entry_id: &str) -> Result<(), Status> {
    let var_name = to_cstring16(ONESHOT_VAR_NAME)?;
    let data = utf16_nul_terminated_bytes(entry_id);

    let attrs = VariableAttributes::BOOTSERVICE_ACCESS | VariableAttributes::RUNTIME_ACCESS;

    runtime::set_variable(var_name.as_ref(), &SYSTEMD_BOOT_LOADER_VENDOR, attrs, &data)
        .map_err(|e| e.status())
}

fn chainload_next(image_handle: Handle, chainload_path: &str) -> Result<(), Status> {
    let loaded_image = boot::open_protocol_exclusive::<LoadedImage>(image_handle)
        .map_err(|e| e.status())?;
    let device_handle = loaded_image.device().ok_or(Status::UNSUPPORTED)?;
    let device_path = boot::open_protocol_exclusive::<DevicePath>(device_handle)
        .map_err(|e| e.status())?;

    let chainload_cstr = to_cstring16(chainload_path)?;
    let mut file_path_buf = Vec::new();
    let file_path = DevicePathBuilder::with_vec(&mut file_path_buf)
        .push(&media::FilePath {
            path_name: chainload_cstr.as_ref(),
        })
        .map_err(|_| Status::OUT_OF_RESOURCES)?
        .finalize()
        .map_err(|_| Status::OUT_OF_RESOURCES)?;

    let file_node = file_path.node_iter().next().ok_or(Status::LOAD_ERROR)?;
    let full_device_path = device_path
        .append_node(file_node)
        .map_err(|_| Status::OUT_OF_RESOURCES)?;

    let loaded = boot::load_image(
        image_handle,
        boot::LoadImageSource::FromDevicePath {
            device_path: &full_device_path,
            boot_policy: BootPolicy::ExactMatch,
        },
    )
    .map_err(|e| e.status())?;

    boot::start_image(loaded).map_err(|e| e.status())
}

fn read_text_file(fs: &mut SimpleFileSystem, path: &str) -> Result<String, Status> {
    let bytes = read_binary_file(fs, path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_binary_file(fs: &mut SimpleFileSystem, path: &str) -> Result<Vec<u8>, Status> {
    let path = to_cstring16(path)?;
    let mut root = fs.open_volume().map_err(|e| e.status())?;
    let handle = root
        .open(path.as_ref(), FileMode::Read, FileAttribute::empty())
        .map_err(|e| e.status())?;

    let mut file = match handle.into_type().map_err(|e| e.status())? {
        FileType::Regular(file) => file,
        _ => return Err(Status::UNSUPPORTED),
    };

    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];

    loop {
        let read = file.read(&mut chunk).map_err(|e| e.status())?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }

    Ok(bytes)
}

fn parse_config(text: &str) -> Result<Config, Status> {
    let kv = parse_key_values(text);
    let chainload_path = get_required_value(&kv, "chainload_path")?.to_owned();
    let entries_path = get_optional_value(&kv, "entries_path")
        .unwrap_or(DEFAULT_ENTRIES_PATH)
        .to_owned();
    Ok(Config {
        chainload_path,
        entries_path,
    })
}

fn parse_choice(text: &str) -> Result<Choice, Status> {
    let kv = parse_key_values(text);
    let mode = get_required_value(&kv, "mode")?;

    match mode {
        "entry" => Ok(Choice::Entry(get_required_value(&kv, "entry")?.to_owned())),
        "nixos-current" => Ok(Choice::NixosCurrent),
        _ => Err(Status::LOAD_ERROR),
    }
}

fn parse_key_values(text: &str) -> Vec<(String, String)> {
    let mut kv = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key.trim(), value.trim()),
            None => continue,
        };

        if key.is_empty() || value.is_empty() {
            continue;
        }

        kv.push((key.to_owned(), value.to_owned()));
    }

    kv
}

fn get_required_value<'a>(kv: &'a [(String, String)], key: &str) -> Result<&'a str, Status> {
    for (k, v) in kv {
        if k == key {
            return Ok(v.as_str());
        }
    }
    Err(Status::LOAD_ERROR)
}

fn get_optional_value<'a>(kv: &'a [(String, String)], key: &str) -> Option<&'a str> {
    for (k, v) in kv {
        if k == key {
            return Some(v.as_str());
        }
    }
    None
}

fn parse_nixos_generation_name(name: &str) -> Option<(u64, String)> {
    const PREFIX: &str = "nixos-generation-";
    const SUFFIX: &str = ".conf";

    if !name.starts_with(PREFIX) || !name.ends_with(SUFFIX) {
        return None;
    }

    let number = &name[PREFIX.len()..name.len() - SUFFIX.len()];
    if number.is_empty() {
        return None;
    }

    let generation = number.parse::<u64>().ok()?;
    let entry_id = name[..name.len() - SUFFIX.len()].to_owned();
    Some((generation, entry_id))
}

fn to_cstring16(s: &str) -> Result<CString16, Status> {
    CString16::try_from(s).map_err(|_| Status::INVALID_PARAMETER)
}

fn utf16_nul_terminated_bytes(value: &str) -> Vec<u8> {
    let mut code_units: Vec<u16> = value.encode_utf16().collect();
    code_units.push(0);

    let mut bytes = Vec::with_capacity(code_units.len() * 2);
    for code_unit in code_units {
        bytes.extend_from_slice(&code_unit.to_le_bytes());
    }
    bytes
}
