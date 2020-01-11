![SmartSTROM Logo](http://www.smartwire.ch/wp-content/uploads/2015/01/digitalstrom.jpg)

# digitalSTROM Server Api in Rust ⚡
This repository provides a digitalStrom Server API as well as a Command Line Interface to control your installation directly.

### Library goals
* Small footporint library
* Easy to use API for digitalStrom Server
* Easy to use CLI for controlling the digitalStrom server
* Open to contribute for everyone

---

# Usage of the API
Add `yahoo-weather-rs` as a dependency in `Cargo.toml`:
```toml
[dependencies]
yahoo-weather = "0.2"
```

```rust
extern crate dss;

fn main() {
    // Connect to the digital strom server
    let  appt = dss::Appartement::connect("url", "user", "password").unwrap();

    // Get an overview of the complete appartment
    println!("{:#?}\n", appt.get_zones().unwrap().iter().find(|z| z.id == zone));

    // turn the light in the zone 2 and group 0 on
    appt.set_value(2, Some(0), dss::Value::Light(1.0)).unwrap();
}
```

# Usage of the CLI
1. Install the CLI in your terminal by `cargo install dss`.
2. Run the CLI
    * Run the CLI by typing `dss` and follow the login instructions
    * Run the CLI by typing `dss server user password` to login automatically
3. Type `zones` to get an overview of your apprtment
4. Type `light on 2` 2 is here the zoneId 

# Contributing
Please contribute! 

The goal is to make this library as usefull as possible :)

If you need any kind of help, open an issue or write me an mail.
Pull requests are welcome!

---
# License
Copyright © 2020 Robert Schütte

Distributed under the [MIT License](LICENSE).