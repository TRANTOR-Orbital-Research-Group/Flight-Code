## Setup 
This is just what I did to get this all setup for running.
Keep in mind that I am on NixOS (Linux) and so the setup might not be exactly the same for someone on Windows.

For NixOS users, theoretically, you should just need to run `nix develop` and then you can upload with `cargo run`.

Other users, it will be slightly weirder. 
You should be able to follow the steps in the [book](https://pico.implrust.com/setup.html).
Then you can get into the SimpleProject (`cd SimpleProject`) directory and run `cargo run` to build it and flash it.