#![no_main]
#![no_std]

extern crate alloc;

mod helpers;
mod config;
mod nixos;

use alloc::vec::Vec;
use config::{find_choice, load_config, Choice};
use helpers::*;
use log::{error, info, warn};
use nixos::find_latest_nixos_entry;
use uefi::prelude::*;
use uefi::proto::device_path::build::{media, DevicePathBuilder};
use uefi::proto::device_path::DevicePath;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::BootPolicy;
use uefi::runtime::{VariableAttributes, VariableVendor};
use uefi::{boot, guid, runtime};

const ONESHOT_VAR_NAME: &str = "LoaderEntryOneShot";
const SYSTEMD_BOOT_LOADER_GUID: uefi::Guid = guid!("4a67b082-0a4c-41cf-b6c7-440b29bb8c4f");
const SYSTEMD_BOOT_LOADER_VENDOR: VariableVendor = VariableVendor(SYSTEMD_BOOT_LOADER_GUID);

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
    info!("scanning filesystems...");

    let resolved_entry = match find_choice(image_handle) {
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

fn set_oneshot_variable(entry_id: &str) -> Result<(), Status> {
    let var_name = to_cstring16(ONESHOT_VAR_NAME)?;
    let data = utf16_nul_terminated_bytes(entry_id);

    let attrs = VariableAttributes::BOOTSERVICE_ACCESS | VariableAttributes::RUNTIME_ACCESS;

    runtime::set_variable(var_name.as_ref(), &SYSTEMD_BOOT_LOADER_VENDOR, attrs, &data)
        .map_err(|e| e.status())
}

fn chainload_next(image_handle: Handle, chainload_path: &str) -> Result<(), Status> {
    // Don't keep exclusive protocol guards alive across StartImage; downstream
    // bootloaders/stubs may need to open filesystem protocols on the same device.
    let full_device_path = {
        let loaded_image = boot::open_protocol_exclusive::<LoadedImage>(image_handle)
            .map_err(|e| e.status())?;
        let device_handle = loaded_image.device().ok_or(Status::UNSUPPORTED)?;
        let device_path =
            boot::open_protocol_exclusive::<DevicePath>(device_handle).map_err(|e| e.status())?;

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
