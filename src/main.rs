use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::Duration;

use btleplug::api::{Central, CharPropFlags, Manager as _, Peripheral, ScanFilter};
use btleplug::platform::Manager;
use chrono::Local;
use clap::Parser;
use futures::stream::StreamExt;
use tokio::time;
use uuid::Uuid;

// BLE
/// Only devices whose name contains this string will be tried.
const PERIPHERAL_NAME_MATCH_FILTER: &str = "M5Atom-MCP3424 BLE Sender";
/// UUID of the characteristic for which we should subscribe to notifications.
const NOTIFY_CHARACTERISTIC_UUID: Uuid = Uuid::from_u128(0xae84d642_7f4b_11ec_a8a3_0242ac120002);

#[derive(Parser)]
#[clap(version = "1.2.0", author = "suzuki_ta")]
struct Opts {
    /// [DEBUG] Only calculate ADC code to Shunt current
    #[clap(short = 'c')]
    calc: Option<f64>, // -c

    /// BLE mode
    #[clap(short = 'B')]
    mode_ble: bool, // -B

    /// UART BAUDRATE
    #[clap(short = 'b', default_value = "115200")]
    uart_baud: u32, // -b

    /// UART interface ex) COM0 ex) /dev/tty.usbserialxxx
    #[clap(short = 'i')]
    uart_interface: Option<String>, // -i

    /// Output file dir.
    #[clap(short, long)]
    output: Option<String>, // -o

    /// Print verbose in console
    #[clap(short)]
    verbose: bool, // -v

    /// ADC bit
    #[clap(short, long, default_value = "12")]
    adc_bit: f64, // -a

    /// Shunt registance[mΩ]
    #[clap(short, long, default_value = "2")]
    shunt_registance: f64, // -s

    /// Reference Voltage[V]
    #[clap(long, default_value = "0.2")]
    refference_voltage: f64,

    /// Amp gain
    #[clap(long, default_value = "100")]
    gain_amp: f64,

    /// Upper voltage divider resistance[Ω]
    #[clap(long, default_value = "3300")]
    upper_resistance: f64,

    /// Lower voltage divider resistance[Ω]
    #[clap(long, default_value = "5600")]
    lower_resistance: f64,

    /// ADC full scale voltage[V]
    #[clap(long, default_value = "2.048")]
    adc_max_voltage: f64,

    /// Enable 2~4ch
    #[clap(short = 'f', long)]
    is_enable_4ch: bool, // -f
}

fn write_data(str: String, opts: &Opts, writer: &mut Option<BufWriter<File>>) {
    if opts.verbose {
        println!("{}, {}", Local::now(), str);
    }
    if let Some(w) = writer {
        w.write_all(format!("{}, {}\n", Local::now(), str).as_bytes())
            .unwrap();
        w.flush().unwrap();
    }
}

fn calc_shunt_current(adc_code: f64, opts: &Opts) -> f64 {
    let v_adc_max: f64 = opts.adc_max_voltage;
    let r_upper: f64 = opts.upper_resistance;
    let r_lower: f64 = opts.lower_resistance;
    let v_ref: f64 = opts.refference_voltage;
    let g_amp: f64 = opts.gain_amp;
    let bit: f64 = opts.adc_bit;
    let shunt: f64 = opts.shunt_registance * 1000.0; // [mΩ] to [Ω]

    let bit_scale: f64 = 2f64.powf(bit - 1.0) - 1.0; // 分解能
    let v_adc = v_adc_max * adc_code / bit_scale;
    let v_o_amp = v_adc * ((r_lower + r_upper) / r_lower);
    let v_i_amp = (v_o_amp - v_ref) / g_amp;
    let i_sr = v_i_amp / shunt;

    println!("adc => {}", adc_code);
    println!("ch1 => {}", i_sr);

    i_sr
}

fn parse_data(str: String, opts: &Opts) -> String {
    let mut str_result: String = String::new();

    let str_arr: Vec<&str> = str.split(',').collect();

    //println!("{:?}", str_arr);

    let len = str_arr.len();
    if len > 1 {
        let adc_code: f64 = str_arr[1].trim().parse().unwrap(); // 受信値
        let i_sr: f64 = calc_shunt_current(adc_code, opts);

        str_result = format!("{}, {}", adc_code, i_sr);
    } else {
        eprintln!("index error");
    }

    if opts.is_enable_4ch {
        if len > 4 {
            let adc_code: f64 = str_arr[2].trim().parse().unwrap(); // 受信値
            let i_sr: f64 = calc_shunt_current(adc_code, opts);

            str_result.push_str(&format!(", {}", i_sr));

            let adc_code: f64 = str_arr[3].trim().parse().unwrap(); // 受信値
            let i_sr: f64 = calc_shunt_current(adc_code, opts);

            str_result.push_str(&format!(", {}", i_sr));

            let adc_code: f64 = str_arr[4].trim().parse().unwrap(); // 受信値
            let i_sr: f64 = calc_shunt_current(adc_code, opts);

            str_result.push_str(&format!(", {}", i_sr));
        } else {
            println!("ch2~4 val error");
        }
    }

    str_result
}

async fn ble_mode(opts: &Opts, writer: &mut Option<BufWriter<File>>) -> Result<(), Box<dyn Error>> {
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
                            let str = String::from_utf8(data.value).unwrap();
                            write_data(parse_data(str, opts), opts, writer);
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

fn uart_mode(opts: &Opts, writer: &mut Option<BufWriter<File>>) {
    let baud: u32 = opts.uart_baud;

    // UART interface指定されていれば正常処理。なければポート表示
    if let Some(ref com) = opts.uart_interface {
        // シリアルポートOPEN
        let mut port = serialport::new(com, baud)
            .timeout(Duration::from_millis(30))
            .open()
            .expect("Failed to open port");

        // UART mode main loop
        loop {
            let mut reader = BufReader::new(&mut port);
            let mut my_str = String::new();
            reader.read_line(&mut my_str).unwrap();

            write_data(parse_data(my_str, opts), opts, writer);
        }
    } else {
        // 検出シリアルポート一覧表示
        println!("=======================");
        println!("<-i> option is required.");
        let ports = serialport::available_ports().expect("No ports found!");
        println!("Available ports: ");
        for p in ports {
            println!("  {}", p.port_name);
        }
        println!("=======================");

        // exit
        std::process::exit(0);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // オプションパース
    let opts: Opts = Opts::parse();
    let mut writer: Option<BufWriter<File>> = None;

    if let Some(c) = &opts.calc {
        let val = parse_data(format!("debug, {}", c), &opts);
        println!("shunt current: {}", val);

        // exit
        std::process::exit(0);
    }

    if let Some(output_dir) = &opts.output {
        fs::create_dir_all(&output_dir)?;

        let output_path = Path::new(&output_dir);
        let output_file = output_path.join(format!(
            "current_{}.txt",
            Local::now().format("%Y%m%d_%H%M%S_%Z")
        ));
        writer = Some(BufWriter::new(File::create(output_file).unwrap()));
    }

    if opts.mode_ble {
        // BLE mode
        ble_mode(&opts, &mut writer).await?;
    } else {
        // UART mode
        uart_mode(&opts, &mut writer);
    }
    Ok(())
}
