# Examples

With Rust, there is great support for examples inside packages and crates. 
For us, this means that we can move examples to this folder and be able to run them whenever we want to check electrical connections.

## Usage
To run whatever example you want, just connect the Pico 2 like normal and run `cargo run --example file_name`
Note that `file_name` excludes the .rs suffix. For example, to upload `blinky.rs`, you would run `cargo run --example blinky`