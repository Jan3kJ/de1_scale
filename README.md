A scale that runs rust because of reasons

## setup
setup esp-rust-toolchain: run outside of this repository
``` shell
cargo install espup --locked
espup install
cargo install espflash --locked
```  
restart terminal/ide after and maybe inbetween installation (due to path updates)

build project
``` shell
cargo build --release
```

after successful build, connect esp32 board and run
``` shell
espflash board-info
```
to search for devices. 

to flash and run the calibration: 
``` shell
cargo run --bin calibrate
```

to flash and run scale
``` shell
cargo run --bin scale
```

