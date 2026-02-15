#![no_main]
#![no_std]

extern crate alloc;

mod helpers;
mod config;
mod fs;
mod nixos;
mod tftp;

use alloc::vec::Vec;
use alloc::string::String;
use config::{Choice, ChoiceSource, Config, load_config};
use fs::find_fs_choice;
use helpers::*;
use log::{error, info, warn};
use nixos::find_latest_nixos_entry;
use tftp::find_tftp_choice;
use uefi::prelude::*;
use uefi::proto::device_path::build::{DevicePathBuilder, media as build_media};
use uefi::proto::device_path::{DevicePath, media as dp_media};
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::BootPolicy;
use uefi::runtime::{VariableAttributes, VariableVendor};
use uefi::{boot, guid, runtime};

const ONESHOT_VAR_NAME: &str = "LoaderEntryOneShot";
const SYSTEMD_BOOT_LOADER_GUID: uefi::Guid = guid!("4a67b082-0a4c-41cf-b6c7-440b29bb8c4f");
const SYSTEMD_BOOT_LOADER_VENDOR: VariableVendor = VariableVendor(SYSTEMD_BOOT_LOADER_GUID);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChainloadDeviceSelector {
    PartitionGuid(uefi::Guid),
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
    connect_all_controllers();

    match oneshot_variable_exists() {
        Ok(true) => {
            info!("{} already set; skipping choice lookup", ONESHOT_VAR_NAME);
        }
        Ok(false) => {
            if lookup_choice_and_set_oneshot(image_handle, &config) {
                return Ok(());
            }
        }
        Err(err) => {
            warn!(
                "failed to check {} ({:?}); continuing with choice lookup",
                ONESHOT_VAR_NAME, err
            );
            if lookup_choice_and_set_oneshot(image_handle, &config) {
                return Ok(());
            }
        }
    }

    info!("chainloading {}", config.chainload_systemd_path);
    chainload_next(image_handle, &config.chainload_systemd_path)?;
    Ok(())
}

fn connect_all_controllers() {
    let handles = match boot::locate_handle_buffer(boot::SearchType::AllHandles) {
        Ok(handles) => handles,
        Err(err) => {
            warn!("failed to list handles for connect_controller ({:?})", err.status());
            return;
        }
    };

    for &handle in handles.iter() {
        if let Err(err) = boot::connect_controller(handle, None, None, true) {
            match err.status() {
                Status::ALREADY_STARTED | Status::NOT_FOUND | Status::ACCESS_DENIED | Status::UNSUPPORTED => {}
                status => warn!("connect_controller failed ({:?})", status),
            }
        }
    }
}

fn resolve_choice_to_entry_id(image_handle: Handle, choice_result: Result<Choice, Status>) -> Option<String> {
    match choice_result {
        Ok(Choice::Entry(entry_id)) => {
            info!("parsed entry id: {}", entry_id);
            Some(entry_id)
        }
        Ok(Choice::NixosCurrent) => match find_latest_nixos_entry(image_handle) {
            Ok(entry_id) => {
                info!("resolved nixos-current to entry id: {}", entry_id);
                Some(entry_id)
            }
            Err(err) => {
                warn!("failed to resolve nixos-current ({:?}); skipping variable set", err);
                None
            }
        },
        Ok(Choice::Chainload(choice_type)) => {
            info!("choice {} does not set {}", choice_type, ONESHOT_VAR_NAME);
            None
        }
        Err(err) => {
            warn!("choice lookup failed ({:?}); skipping variable set", err);
            None
        }
    }
}

fn lookup_choice(image_handle: Handle, config: &Config) -> Result<Choice, Status> {
    match config.choice_source {
        ChoiceSource::Fs => {
            info!("choice source: fs");
            find_fs_choice(image_handle)
        }
        ChoiceSource::Tftp => {
            let tftp_ip = config.tftp_ip.as_deref().ok_or(Status::LOAD_ERROR)?;
            info!("choice source: tftp ({})", tftp_ip);
            find_tftp_choice(tftp_ip)
        }
    }
}

fn try_chainload_choice(
    image_handle: Handle,
    config: &Config,
    choice_result: &Result<Choice, Status>,
) -> bool {
    let choice_type = match choice_result {
        Ok(Choice::Chainload(choice_type)) => choice_type,
        _ => return false,
    };

    let chainload_path = match config.chainload_path_for_mode(choice_type) {
        Some(path) => path,
        None => {
            warn!(
                "choice {} requested, but {}_path is missing in {}; falling back to systemd chainload",
                choice_type, choice_type, crate::config::CONFIG_PATH
            );
            return false;
        }
    };

    info!("choice {} requested direct chainload", choice_type);
    match chainload_next(image_handle, chainload_path) {
        Ok(()) => true,
        Err(err) => {
            warn!(
                "direct chainload for {} failed ({:?}); falling back to systemd chainload",
                choice_type, err
            );
            false
        }
    }
}

fn lookup_choice_and_set_oneshot(image_handle: Handle, config: &Config) -> bool {
    let choice_result = lookup_choice(image_handle, config);

    if try_chainload_choice(image_handle, config, &choice_result) {
        return true;
    }

    let resolved_entry = resolve_choice_to_entry_id(image_handle, choice_result);

    if let Some(entry_id) = resolved_entry {
        match set_oneshot_variable(&entry_id) {
            Ok(()) => info!("set {} OK", ONESHOT_VAR_NAME),
            Err(err) => warn!("failed to set {} ({:?})", ONESHOT_VAR_NAME, err),
        }
    }

    false
}

fn set_oneshot_variable(entry_id: &str) -> Result<(), Status> {
    let var_name = to_cstring16(ONESHOT_VAR_NAME)?;
    let data = utf16_nul_terminated_bytes(entry_id);

    let attrs = VariableAttributes::BOOTSERVICE_ACCESS | VariableAttributes::RUNTIME_ACCESS;

    runtime::set_variable(var_name.as_ref(), &SYSTEMD_BOOT_LOADER_VENDOR, attrs, &data)
        .map_err(|e| e.status())
}

fn oneshot_variable_exists() -> Result<bool, Status> {
    let var_name = to_cstring16(ONESHOT_VAR_NAME)?;
    runtime::variable_exists(var_name.as_ref(), &SYSTEMD_BOOT_LOADER_VENDOR).map_err(|e| e.status())
}

fn parse_chainload_selector(prefix: &str) -> Result<ChainloadDeviceSelector, Status> {
    let guid = uefi::Guid::try_parse(prefix.trim()).map_err(|_| Status::LOAD_ERROR)?;
    Ok(ChainloadDeviceSelector::PartitionGuid(guid))
}

fn normalize_chainload_file_path(path: &str) -> Result<String, Status> {
    let mut path = normalize_uefi_path(path.trim());
    if path.is_empty() {
        return Err(Status::LOAD_ERROR);
    }
    if !path.starts_with('\\') {
        path.insert(0, '\\');
    }
    Ok(path)
}

fn parse_chainload_target(path: &str) -> Result<(Option<ChainloadDeviceSelector>, String), Status> {
    if let Some((prefix, file_path)) = path.split_once(':') {
        let selector = parse_chainload_selector(prefix)?;
        let file_path = normalize_chainload_file_path(file_path)?;
        return Ok((Some(selector), file_path));
    }

    let file_path = normalize_chainload_file_path(path)?;
    Ok((None, file_path))
}

fn source_device_handle(image_handle: Handle) -> Result<Handle, Status> {
    let loaded_image = boot::open_protocol_exclusive::<LoadedImage>(image_handle)
        .map_err(|e| e.status())?;
    loaded_image.device().ok_or(Status::UNSUPPORTED)
}

fn extract_partition_guid(device_path: &DevicePath) -> Option<uefi::Guid> {
    for node in device_path.node_iter() {
        if let Ok(hard_drive) = <&dp_media::HardDrive>::try_from(node) {
            if hard_drive.partition_format() != dp_media::PartitionFormat::GPT {
                return None;
            }

            if let dp_media::PartitionSignature::Guid(partition_guid) = hard_drive.partition_signature() {
                return Some(partition_guid);
            }

            return None;
        }
    }
    None
}

fn resolve_guid_device_handle(partition_guid: uefi::Guid) -> Result<Handle, Status> {
    let handles = boot::find_handles::<SimpleFileSystem>().map_err(|e| e.status())?;

    for handle in handles {
        let device_path = match boot::open_protocol_exclusive::<DevicePath>(handle) {
            Ok(device_path) => device_path,
            Err(_) => continue,
        };

        let current_guid = match extract_partition_guid(&device_path) {
            Some(guid) => guid,
            None => continue,
        };

        if current_guid == partition_guid {
            return Ok(handle);
        }
    }

    Err(Status::NOT_FOUND)
}

fn resolve_chainload_device_handle(
    image_handle: Handle,
    selector: Option<ChainloadDeviceSelector>,
) -> Result<Handle, Status> {
    match selector {
        None => source_device_handle(image_handle),
        Some(ChainloadDeviceSelector::PartitionGuid(partition_guid)) => {
            resolve_guid_device_handle(partition_guid)
        }
    }
}

fn chainload_next(image_handle: Handle, chainload_systemd_path: &str) -> Result<(), Status> {
    let (selector, file_path) = parse_chainload_target(chainload_systemd_path)?;
    let device_handle = resolve_chainload_device_handle(image_handle, selector)?;
    match selector {
        None => info!("chainload source: current boot device"),
        Some(ChainloadDeviceSelector::PartitionGuid(partition_guid)) => {
            info!("chainload source: partition GUID {}", partition_guid);
        }
    }
    info!("chainload file path: {}", file_path);

    // Don't keep exclusive protocol guards alive across StartImage; downstream
    // bootloaders/stubs may need to open filesystem protocols on the same device.
    let full_device_path = {
        let device_path =
            boot::open_protocol_exclusive::<DevicePath>(device_handle).map_err(|e| e.status())?;

        let chainload_cstr = to_cstring16(&file_path)?;
        let mut file_path_buf = Vec::new();
        let file_path = DevicePathBuilder::with_vec(&mut file_path_buf)
            .push(&build_media::FilePath {
                path_name: chainload_cstr.as_ref(),
            })
            .map_err(|_| Status::OUT_OF_RESOURCES)?
            .finalize()
            .map_err(|_| Status::OUT_OF_RESOURCES)?;

        let file_node = file_path.node_iter().next().ok_or(Status::LOAD_ERROR)?;
        device_path
            .append_node(file_node)
            .map_err(|_| Status::OUT_OF_RESOURCES)?
    };

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
