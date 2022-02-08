use std::error::Error;
use std::time::Duration;

use btleplug::api::{Central, CharPropFlags, Manager as _, Peripheral, ScanFilter};
use btleplug::platform::Manager;
use chrono::Local;
use futures::stream::StreamExt;
use tokio::time;
use uuid::Uuid;

// BLE
/// Only devices whose name contains this string will be tried.
const PERIPHERAL_NAME_MATCH_FILTER: &str = "M5Atom-MCP3424 BLE Sender";
/// UUID of the characteristic for which we should subscribe to notifications.
const NOTIFY_CHARACTERISTIC_UUID: Uuid = Uuid::from_u128(0xae84d642_7f4b_11ec_a8a3_0242ac120002);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Hello, world!");
    // BLE mode
    let manager = Manager::new().await?;
    // get 'Central' BLE adapter list
    let adapter_list = manager.adapters().await?;
    if adapter_list.is_empty() {
        eprintln!("No BLE adapters found.");
    }

    'adapter_loop: for adapter in adapter_list.iter() {
        // println!("{:?}", adapter);
        println!("Scanning...");

        // Start scanning 'Peripheral'
        adapter
            .start_scan(ScanFilter::default())
            .await
            .expect("Can't scan BLE adapter for connected devices..");
        time::sleep(Duration::from_secs(2)).await;
        // get peripherals list
        let peripherals = adapter.peripherals().await?;
        if peripherals.is_empty() {
            eprintln!("  BLE peripheral devices were not found.");
        } else {
            for peripheral in peripherals.iter() {
                let properties = peripheral.properties().await?;
                let is_connected = peripheral.is_connected().await?;
                let local_name = properties
                    .unwrap()
                    .local_name
                    .unwrap_or(String::from("Unknown Peripheral"));
                println!(
                    "Peripheral {:?} is connected: {:?}",
                    &local_name, is_connected
                );
                if local_name.contains(PERIPHERAL_NAME_MATCH_FILTER) {
                    println!("Found matching peripheral {:?}", &local_name);
                    if !is_connected {
                        // Connection
                        if let Err(err) = peripheral.connect().await {
                            eprintln!("Error connecting to peripheral, skipping: {}", err);
                            continue;
                        }
                    }
                    let is_connected = peripheral.is_connected().await?;
                    println!(
                        "Now connected ({:?}) to peripheral {:?}.",
                        is_connected, &local_name
                    );
                    if is_connected {
                        // services
                        peripheral.discover_services().await?;
                        // characteristics
                        let characteristics = peripheral.characteristics();

                        let notify_chara = characteristics
                            .iter()
                            .find(|&c| {
                                c.uuid == NOTIFY_CHARACTERISTIC_UUID
                                    && c.properties.contains(CharPropFlags::NOTIFY)
                            })
                            .expect("Notify characteristic is not found");
                        // Notify
                        println!("Subscribing to characteristic {:?}", notify_chara.uuid);
                        peripheral.subscribe(&notify_chara).await?;
                        let mut notification_stream = peripheral.notifications().await?;
                        while let Some(data) = notification_stream.next().await {
                            println!(
                                "Received:, {}, {}",
                                Local::now(),
                                String::from_utf8(data.value).unwrap()
                            );
                        }
                        // Disconnect
                        println!("Disconnecting from peripheral {:?}", local_name);
                        peripheral.disconnect().await?;
                    }
                    // End..
                    break 'adapter_loop;
                } else {
                    println!("Skipping peripheral: {:?}", peripheral);
                }
            }
        }
    }
    Ok(())
}
