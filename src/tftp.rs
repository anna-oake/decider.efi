use alloc::string::String;
use alloc::vec::Vec;
use core::net::IpAddr;
use log::{info, warn};
use uefi::boot;
use uefi::proto::network::pxe::BaseCode;
use uefi::{CStr8, Handle, Status};

use crate::config::Choice;
use crate::helpers::parse_choice;

const CHOICE_FILENAME: &str = "DECIDER.CHO";

pub fn find_tftp_choice(tftp_ip: &str) -> Result<Choice, Status> {
    let server_ip = parse_tftp_ip(tftp_ip)?;
    info!("tftp target server ip: {}", server_ip);

    let handles = match boot::find_handles::<BaseCode>() {
        Ok(handles) => handles,
        Err(err) => {
            warn!("failed to discover PXE BaseCode handles: {:?}", err.status());
            return Err(err.status());
        }
    };
    info!("discovered {} PXE handles", handles.len());
    if handles.is_empty() {
        warn!("no PXE BaseCode handles found");
    }

    let mut last_error = Status::NOT_FOUND;

    for (idx, handle) in handles.into_iter().enumerate() {
        info!("trying PXE handle #{}", idx);
        match fetch_choice_from_handle(handle, server_ip) {
            Ok(choice) => return Ok(choice),
            Err(err) => {
                warn!("skipping PXE handle #{} (tftp failed: {:?})", idx, err);
                last_error = err;
            }
        }
    }

    Err(last_error)
}

fn parse_tftp_ip(raw: &str) -> Result<IpAddr, Status> {
    let ip = raw.trim();
    match ip.parse::<IpAddr>() {
        Ok(ip) => Ok(ip),
        Err(_) => {
            warn!("invalid tftp_ip '{}': expected IPv4/IPv6 literal", raw);
            Err(Status::INVALID_PARAMETER)
        }
    }
}

fn fetch_choice_from_handle(handle: Handle, server_ip: IpAddr) -> Result<Choice, Status> {
    let mut pxe = match boot::open_protocol_exclusive::<BaseCode>(handle) {
        Ok(pxe) => pxe,
        Err(err) => {
            warn!("failed to open PXE BaseCode protocol: {:?}", err.status());
            return Err(err.status());
        }
    };

    let already_started = pxe.mode().started();
    info!("pxe started={}", already_started);
    info!(
        "pxe mode: using_ipv6={}, ipv6_supported={}, ipv6_available={}, station_ip={}",
        pxe.mode().using_ipv6(),
        pxe.mode().ipv6_supported(),
        pxe.mode().ipv6_available(),
        pxe.mode().station_ip()
    );

    if !already_started {
        info!("starting pxe");
        if let Err(err) = pxe.start(false) {
            warn!("pxe start failed: {:?}", err.status());
            return Err(err.status());
        }
    }

    let result = fetch_choice_with_pxe(&mut pxe, server_ip);

    if !already_started {
        info!("stopping pxe");
        if let Err(err) = pxe.stop() {
            warn!("pxe stop failed: {:?}", err.status());
        }
    }

    result
}

fn fetch_choice_with_pxe(pxe: &mut BaseCode, server_ip: IpAddr) -> Result<Choice, Status> {
    if !pxe.mode().dhcp_ack_received() {
        info!("running pxe dhcp");
        if let Err(err) = pxe.dhcp(false) {
            warn!("pxe dhcp failed: {:?}", err.status());
            return Err(err.status());
        }
    } else {
        info!("pxe dhcp already completed");
    }
    info!("pxe station ip: {}", pxe.mode().station_ip());

    info!("fetching choice over tftp from {}:{}", server_ip, CHOICE_FILENAME);

    let mut c_path = CHOICE_FILENAME.as_bytes().to_vec();
    c_path.push(0);
    let filename = CStr8::from_bytes_with_nul(&c_path).map_err(|_| Status::INVALID_PARAMETER)?;

    let file_size = match pxe.tftp_get_file_size(&server_ip, filename) {
        Ok(size) => size,
        Err(err) => {
            warn!("tftp_get_file_size failed: {:?}", err.status());
            return Err(err.status());
        }
    };
    info!("tftp file size for {}: {}", CHOICE_FILENAME, file_size);

    let buffer_len = usize::try_from(file_size).map_err(|_| Status::OUT_OF_RESOURCES)?;
    let mut file_data = Vec::new();
    file_data.resize(buffer_len, 0);

    let bytes_read = match pxe.tftp_read_file(&server_ip, filename, Some(&mut file_data)) {
        Ok(read) => read,
        Err(err) => {
            warn!("tftp_read_file failed: {:?}", err.status());
            return Err(err.status());
        }
    };
    let bytes_read = usize::try_from(bytes_read).map_err(|_| Status::LOAD_ERROR)?;
    file_data.truncate(bytes_read);

    let choice_text = String::from_utf8_lossy(&file_data);
    parse_choice(&choice_text).map_err(|err| {
        warn!("failed to parse tftp choice payload: {:?}", err);
        err
    })
}
