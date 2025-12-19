#![no_main]
#![no_std]

use uefi::prelude::*;
use core::time::Duration;
use log::info;

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    info!("Hello world!");
    boot::stall(Duration::from_mins(10));
    Status::SUCCESS
}