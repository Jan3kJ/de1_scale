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

// helper: busy-wait sleep using time::Instant (avoids using Delay which may be moved)
fn busy_wait_ms(ms: u64) {
	let start = time::Instant::now().duration_since_epoch().as_millis();
	let target = start + (ms as u64);
	while time::Instant::now().duration_since_epoch().as_millis() < target {
		// spin
	}
}

// helper: average N corrected_value() samples with a small busy-wait between reads
fn avg_reading(scale: &mut Scale, samples: usize) -> f32 {
	let mut sum: f32 = 0.0;
	let mut i = 0;
	while i < samples {
		let v = scale.corrected_value() as f32;
		sum += v;
		// small busy-wait between raw reads (~10 ms)
		busy_wait_ms(10);
		i += 1;
	}
	sum / (samples as f32)
}

// Calibrate a single scale using a known weight in grams.
// baseline is returned only for detection; raw_per_g is computed from loaded (no baseline subtraction)
fn calibrate_scale(scale: &mut Scale, name: &str, known_g: f32) -> (f32, f32) {
	println!("Calibrating {name}: measuring baseline (no weight)...");
	let baseline = avg_reading(scale, DETECT_SAMPLES);
	println!("{} baseline: {}", name, baseline);

	println!("Place {} g reference on the {name} scale now ({}s timeout)...", known_g, DETECT_TIMEOUT_MS / 1000);
	let start_ms = time::Instant::now().duration_since_epoch().as_millis();
	let mut detected = false;
	while time::Instant::now().duration_since_epoch().as_millis().saturating_sub(start_ms) < DETECT_TIMEOUT_MS {
		let sample = avg_reading(scale, DETECT_SAMPLES);
		let delta = (sample - baseline).abs();
		let threshold = (baseline.abs() * 0.1) + 2000.0f32; // tweak as needed
		if delta > threshold {
			detected = true;
			break;
		}
		busy_wait_ms(50);
	}

	if !detected {
		println!("No stable weight detected on {} within timeout. Using fallback.", name);
		return (1.0, baseline);
	}

	println!("Weight detected on {name}, averaging stable readings...");
	let loaded = avg_reading(scale, STABLE_SAMPLES);
	let raw_per_g = loaded / known_g; // DO NOT subtract baseline here; corrected_value() is already tare-corrected
	println!("{name} calibration complete: raw_per_g = {}, loaded = {}, baseline = {}", raw_per_g, loaded, baseline);
	(raw_per_g, baseline)
}

// Calibrate both scales together: detect weight placed across both, then average each scale's readings
// Returns (left_raw_per_g, right_raw_per_g, left_baseline, right_baseline)
fn calibrate_both(left: &mut Scale, right: &mut Scale, known_g: f32) -> f32 {
	println!("Calibrating both scales together: measuring baselines...");
	let left_baseline = avg_reading(left, DETECT_SAMPLES);
	let right_baseline = avg_reading(right, DETECT_SAMPLES);
	let combined_baseline = left_baseline + right_baseline;
	println!("baselines: left={} right={} combined={}", left_baseline, right_baseline, combined_baseline);

	println!("Place {} g reference across both scales now ({}s timeout)...", known_g, DETECT_TIMEOUT_MS / 1000);
	let start_ms = time::Instant::now().duration_since_epoch().as_millis();
	let mut detected = false;
	while time::Instant::now().duration_since_epoch().as_millis().saturating_sub(start_ms) < DETECT_TIMEOUT_MS {
		let l_s = avg_reading(left, DETECT_SAMPLES);
		let r_s = avg_reading(right, DETECT_SAMPLES);
		let combined = l_s + r_s;
		let delta = (combined - combined_baseline).abs();
		let threshold = (combined_baseline.abs() * 0.05) + 2000.0f32; // tweak as needed
		if delta > threshold {
			detected = true;
			break;
		}
		busy_wait_ms(50);
	}

	if !detected {
		println!("No stable combined weight detected within timeout. Using fallbacks.");
		return 1.0;
	}

	println!("Combined weight detected, averaging stable readings for both scales...");
	let loaded_l = avg_reading(left, STABLE_SAMPLES);
	let loaded_r = avg_reading(right, STABLE_SAMPLES);

	// Use corrected_value() directly (tare applied), compute per-scale raw_per_g
	let raw_per_g = (loaded_l + loaded_r) / known_g;

	println!(
		"Both calibration complete: raw_per_g = {}, loaded_l = {}, loaded_r = {}",
		raw_per_g, loaded_l, loaded_r
	);

	raw_per_g
}

#[main]
fn main() -> ! {
    let now = || time::Instant::now().duration_since_epoch().as_millis();
    esp_println::logger::init_logger_from_env();
    let config = hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = hal::init(config);

    let _rtc = Rtc::new(peripherals.LPWR);
    let output_config = OutputConfig::default();

    log::info!("Logger is setup");
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    let _io = Io::new(peripherals.IO_MUX);
    let mut delay = Delay::new();

    let mut led = Output::new(peripherals.GPIO2, Level::Low, output_config);
    led.set_high();
    log::info!("LED on?");

    let floating_config = InputConfig::default();

    let dout = Input::new(peripherals.GPIO16, floating_config);
    let pd_sck = Output::new(peripherals.GPIO4, Level::Low, output_config);
    let mut hx = hx711::Hx711::new(delay, dout, pd_sck).unwrap();

    log::info!("Interrogating some stuff");
    let enabled = hx.enable();
    if let Ok(()) = enabled {
        log::info!("Enalbed");
    }

    let mut left = Scale::new(&mut hx);

    let dout = Input::new(peripherals.GPIO18, floating_config);
    let pd_sck = Output::new(peripherals.GPIO5, Level::Low, output_config);
    let mut hx = hx711::Hx711::new(delay, dout, pd_sck).unwrap();
    let mut right = Scale::new(&mut hx);

    let mut values: Buffer<4> = Buffer::new();
    left.enable();
    right.enable();

    left.tare();
    right.tare();

	// perform combined calibration with a 20 g reference weight
	let known_weight_g: f32 = 20.0;
	let raw_per_g = calibrate_both(&mut left, &mut right, known_weight_g);

    let mut last = now();
    loop {
        let current = now();
        if current.saturating_sub(last) >= UPDATE_INTERVAL_MS {
            let l_raw = left.corrected_value() as f32;
            let r_raw = right.corrected_value() as f32;

            let per_g = if raw_per_g.abs() >= 1e-3 { raw_per_g } else { 1.0 };

            // Do not subtract baseline here; corrected_value() is tare-corrected
            let w = (l_raw + r_raw) / per_g;

            values.push(w);
            println!("l: {l_raw} r: {r_raw} => weight(g): {}", values.average());
            last = current;
        } else {
            // sleep a short while to yield CPU and avoid printing too fast
            delay.delay_ns(50_000_000u32); // 50 ms
        }
    }
}
