use std::io::Write;
use std::str::FromStr;

fn main() -> Result<(), dss::Error> {
    // shown welcome screen
    println!();
    println!("**************************************************************");
    println!("*                 Digital Strom Server - CLI                 *");
    println!("**************************************************************");

    // login
    let appt = login()?;
    println!("*> Success");
    println!("*>");

    // get the event channel and display the messages when they get received
    let events = appt.event_channel()?;
    std::thread::spawn(move || loop {
        let res = events.recv();

        match res {
            Ok(v) => {
                println!("\n{:#?}\n", v);
                print!("*dss> ");
                std::io::stdout()
                    .flush()
                    .expect("Output flush failed - internal error");
            }
            Err(_) => {
                println!("Channel Closed");
                break;
            }
        }
    });

    // CLI loop
    loop {
        let mut cmd = String::from("");
        print!("*dss> ");
        std::io::stdout()
            .flush()
            .expect("Output flush failed - internal error");
        std::io::stdin()
            .read_line(&mut cmd)
            .expect("Please enter a valid String");

        // trim the command
        cmd = cmd.trim().to_lowercase();
        // split the command at white spaces
        let cmds: Vec<&str> = cmd.split_whitespace().collect();

        // show all zones
        if cmd == "zones" {
            println!("{:#?}\n", appt.get_zones());
        // show a specific zone
        } else if cmd.starts_with("zone") {
            // start again if not enough parameter where subimitted
            if cmds.len() < 2 {
                print_zone_help();
                continue;
            }

            // When a conversion to usize works, filter the zones
            if let Ok(zone) = get_room_id(&appt, cmds[1]) {
                println!("{:#?}\n", appt.get_zones()?.iter().find(|z| z.id == zone));
            } else {
                print_zone_help();
            }

        // set the light
        } else if cmd.starts_with("light") | cmd.starts_with("licht") {
            // don't continue if not enough parameters are available
            if cmds.len() < 3 {
                print_light_help();
                continue;
            }

            // interprete the value to set
            let val = get_value(cmds[1]);

            // interpret the group to set
            let group = cmds.get(3).and_then(|g| usize::from_str(g).ok());

            // when the zone can be converted to a number, set the value
            if let Ok(zone) = get_room_id(&appt, cmds[2]) {
                appt.set_value(zone, group, dss::Value::Light(val))?;
            } else {
                print_light_help();
            }
        }
        // set the shadow
        else if cmd.starts_with("shadow") | cmd.starts_with("schatten") {
            // don't continue if not enough parameters are available
            if cmds.len() < 4 {
                print_shadow_help();
                continue;
            }

            // interprete the open value to set
            let open = get_value(cmds[1]);

            // interprete the angle value to set
            let angle = get_value(cmds[2]);

            // interpret the group to set
            let group = cmds.get(4).and_then(|g| usize::from_str(g).ok());

            // when the zone can be converted to a number, set the value
            if let Ok(zone) = get_room_id(&appt, cmds[3]) {
                appt.set_value(zone, group, dss::Value::Shadow(open, angle))?;
            } else {
                print_shadow_help();
            }
        }
        // exit the programm
        else if cmd == "exit" {
            std::process::exit(0);
        // Show the help
        } else if cmd.starts_with("help") | cmd.starts_with("hilfe") {
            // print default help when no parameters are available
            if cmds.is_empty() {
                print_help();
                continue;
            }

            // print specific error
            match cmds[1] {
                "zone" => print_zone_help(),
                "light" => print_light_help(),
                "shadow" => print_shadow_help(),
                _ => print_help(),
            }
        } else {
            // check if not enough parameters are available
            if cmds.len() < 3 {
                print_help();
                continue;
            }

            // if enough is avilable get the room id
            if let Ok(zone) = get_room_id(&appt, cmds[0]) {
                if cmds[1] == "light" || cmds[1] == "licht" {
                    // interprete the value to set
                    let val = get_value(cmds[2]);

                    // set light value
                    appt.set_value(zone, None, dss::Value::Light(val))?;
                } else if cmds[1] == "shadow" || cmds[1] == "schatten" {
                    // check if not enough parameters are available
                    if cmds.len() < 4 {
                        print_help();
                        continue;
                    }

                    // interprete the open value to set
                    let open = get_value(cmds[2]);

                    // interprete the angle value to set
                    let angle = get_value(cmds[3]);

                    // set shadow value
                    appt.set_value(zone, None, dss::Value::Shadow(open, angle))?;
                } else if cmds[1] == "zone" {
                    // show the zone details
                    println!("{:#?}\n", appt.get_zones()?.iter().find(|z| z.id == zone));
                }
            } else {
                print_help();
            }
        }
    }
}

/// Login function either by arguments or manually step by step
fn login() -> Result<dss::Appartement, dss::Error> {
    let mut host = String::from("");
    let mut user = String::from("");
    let mut pass = String::from("");

    // when enviorement parameters are defined try them
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 4 {
        host = args[1].clone();
        user = args[2].clone();
        pass = args[3].clone();
    }
    // or show login dialog to the user
    else {
        println!("*> Please Enter the hostname or ip");
        std::io::stdin()
            .read_line(&mut host)
            .expect("Please enter a valid String");
        println!("*> Please Enter the username");
        std::io::stdin()
            .read_line(&mut user)
            .expect("Please enter a valid String");
        println!("*> Please Enter the password");
        std::io::stdin()
            .read_line(&mut pass)
            .expect("Please enter a valid String");
    }

    println!("*> Please be aware, the login and first fetch can take more than 30 seconds!");
    println!("*>");
    println!("*> Login...");

    // try to login
    dss::Appartement::connect(host.trim(), user.trim(), pass.trim())
}

/// Get the room id either from the room name or id
fn get_room_id(appt: &dss::Appartement, inp: &str) -> Result<usize, dss::Error> {
    // check if an ID is available
    if let Ok(id) = usize::from_str(inp) {
        return Ok(id);
    }

    // get all room names and check for a match
    for room in appt.get_zones()?.iter() {
        if room.name.to_lowercase() == inp {
            return Ok(room.id);
        }
    }

    Err("No matching room found".into())
}

/// Get the value from the input string
fn get_value(inp: &str) -> f32 {
    match inp {
        // check for keywords
        "close" | "on" | "an" | "zu" => 1.0,
        "open" | "off" | "aus" | "auf" => 0.0,
        // try to convert the number
        _ => {
            if let Ok(stat) = f32::from_str(inp) {
                stat
            } else {
                0.0
            }
        }
    }
}

fn print_help() {
    println!("zones     Get all zones and the included data");
    println!("zone      Get a specific zone data");
    println!("light     Set the light for a zone");
    println!("shadow    Set the shadow for a zone");
    println!("exit      Exit the DSS CLI");
    println!("help      Show this help text");
    println!();
}

fn print_zone_help() {
    println!("Please define a valid zone number, like following: ");
    println!("zone office");
    println!("zone 2");
}

fn print_light_help() {
    println!("Please define a valid light command and zone, like the following: ");
    println!("light off office");
    println!("light on 2");
}

fn print_shadow_help() {
    println!("Please define a valid shadow command and zone, like the following: ");
    println!("shadow open open office");
    println!("shadow 0.5 close office");
    println!("shadow open open 2");
}
