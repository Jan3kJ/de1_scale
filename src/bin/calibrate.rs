#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_println::println;
use hal::clock::CpuClock;
use hx711;
use hal::main;


use hal::rtc_cntl::Rtc;
use hal::time;
use hal::timer::timg::TimerGroup;
use hal::delay::Delay;
use embedded_hal::delay::DelayNs;
use hal::gpio::{Input, InputConfig, Io, Level, Output, OutputConfig, Pull};
use scale::{Scale, Buffer};

const UPDATE_INTERVAL_MS: u64 = 1000; // 1000 ms = 1 s
const DETECT_SAMPLES: usize = 5;
const STABLE_SAMPLES: usize = 100;
const DETECT_TIMEOUT_MS: u64 = 30_000; // 30s

esp_bootloader_esp_idf::esp_app_desc!();

// helper: average N corrected_value() samples with small delay between reads
fn avg_reading(scale: &mut Scale, samples: usize, delay: &mut Delay) -> f32 {
	// ...small, deterministic averaging without heap...
	let mut sum: f32 = 0.0;
	let mut i = 0;
	while i < samples {
		let v = scale.corrected_value() as f32;
		sum += v;
		// small sleep between raw reads
		delay.delay_ns(10_000_000u32); // 10 ms
		i += 1;
	}
	sum / (samples as f32)
}

// Calibrate a single scale using a known weight in grams. Returns (raw_per_gram, baseline_raw).
fn calibrate_scale(scale: &mut Scale, delay: &mut Delay, name: &str, known_g: f32) -> (f32, f32) {
	println!("Calibrating {name}: measuring baseline (no weight)...");
	let baseline = avg_reading(scale, DETECT_SAMPLES, delay);
	println!("{} baseline: {}", name, baseline);

	println!("Place {} g reference on the {name} scale now ({}s timeout)...", known_g, DETECT_TIMEOUT_MS / 1000);
	let start_ms = time::Instant::now().duration_since_epoch().as_millis();
	let mut detected = false;
	// wait for a detectable change
	while time::Instant::now().duration_since_epoch().as_millis().saturating_sub(start_ms) < DETECT_TIMEOUT_MS {
		let sample = avg_reading(scale, DETECT_SAMPLES, delay);
		let delta = (sample - baseline).abs();
		// adaptive threshold: absolute or relative small baseline
		let threshold = (baseline.abs() * 0.1) + 2000.0f32; // tweak as needed
		if delta > threshold {
			detected = true;
			break;
		}
		// small sleep to avoid busy loop
		delay.delay_ns(50_000_000u32); // 50 ms
	}

	if !detected {
		println!("No stable weight detected on {} within timeout. Using fallback.", name);
		// avoid divide by zero: return 1.0 so code keeps running (user should retry)
		return (1.0, baseline);
	}

	println!("Weight detected on {name}, averaging stable readings...");
	let loaded = avg_reading(scale, STABLE_SAMPLES, delay);
	let raw_per_g = (loaded - baseline) / known_g;
	println!("{name} calibration complete: raw_per_g = {}, loaded = {}, baseline = {}", raw_per_g, loaded, baseline);
	(raw_per_g, baseline)
}

#[main]
fn main() -> ! {
    let now = || time::Instant::now().duration_since_epoch().as_millis();
    esp_println::logger::init_logger_from_env();
    let config = hal::Config::default().with_cpu_clock(CpuClock::max());
    let mut peripherals = hal::init(config);

    let rtc = Rtc::new(peripherals.LPWR);
    let output_config = OutputConfig::default();

    log::info!("Logger is setup");
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    let mut io = Io::new(peripherals.IO_MUX);
    let mut delay = Delay::new();

    let mut led = Output::new(peripherals.GPIO2, Level::Low, output_config);
    led.set_high();
    log::info!("LED on?");

    let floating_config = InputConfig::default();

    log::info!("setup left scale");
    let dout = Input::new(peripherals.GPIO16, floating_config);
    let pd_sck = Output::new(peripherals.GPIO4, Level::Low, output_config);
    let mut hx = hx711::Hx711::new(delay, dout, pd_sck).unwrap();

    log::info!("Interrogating some stuff");
    let enabled = hx.enable();
    if let Ok(()) = enabled {
        log::info!("Enalbed");
    }

    let mut left = Scale::new(&mut hx);

    log::info!("setup right scale");
    let dout = Input::new(peripherals.GPIO18, floating_config);
    let pd_sck = Output::new(peripherals.GPIO5, Level::Low, output_config);
    let mut hx = hx711::Hx711::new(delay, dout, pd_sck).unwrap();
    let mut right = Scale::new(&mut hx);

    let mut values: Buffer<4> = Buffer::new();
    log::info!("enable scales");
    left.enable();
    right.enable();

    log::info!("tare scales");
    left.tare();
    right.tare();
    log::info!("tare done");

	// perform automatic calibration with a 20 g reference weight
	let known_weight_g: f32 = 20.0;
	let (left_raw_per_g, left_baseline) = calibrate_scale(&mut left, &mut delay, "left", known_weight_g);
	let (right_raw_per_g, right_baseline) = calibrate_scale(&mut right, &mut delay, "right", known_weight_g);

    let mut last = now();
    loop {
        let current = now();
        if current.saturating_sub(last) >= UPDATE_INTERVAL_MS {
            // read and compute using calibrated factors (guard against zero)
            let l_raw = left.corrected_value() as f32;
            let r_raw = right.corrected_value() as f32;

            let l_per_g = if left_raw_per_g.abs() >= core::f32::EPSILON { left_raw_per_g } else { 1.0 };
            let r_per_g = if right_raw_per_g.abs() >= core::f32::EPSILON { right_raw_per_g } else { 1.0 };

            let l_g = (l_raw - left_baseline) / l_per_g;
            let r_g = (r_raw - right_baseline) / r_per_g;
            let w = l_g + r_g;

            values.push(w);
            println!("l: {l_raw} r: {r_raw} => weight(g): {}", values.average());
            last = current;
        } else {
            // sleep a short while to yield CPU and avoid printing too fast
            delay.delay_ns(50_000_000u32); // 50 ms
        }
    }
}
