use std::io::Write;
use std::str::FromStr;

fn main() -> Result<(), dss::Error> {
    // shown welcome screen
    println!("");
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
        cmd = cmd.trim().to_string();

        // show all zones
        if cmd == "zones" {
            println!("{:#?}\n", appt.get_zones());
        // show a specific zone
        } else if cmd.starts_with("zone") {
            // split the command at white spaces
            let c: Vec<&str> = cmd.split_whitespace().collect();

            // start again if not enough parameter where subimitted
            if c.len() < 2 {
                println!("Please define a valid zone number, like following: ");
                println!("zone 2");
                continue;
            }

            // When a conversion to usize works, filter the zones
            if let Ok(zone) = usize::from_str(c[1]) {
                println!("{:#?}\n", appt.get_zones()?.iter().find(|z| z.id == zone));
            } else {
                println!("Please define a valid zone number, like following: ");
                println!("zone 2");
            }

        // set the light
        } else if cmd.starts_with("light") {
            // split the command at white spaces
            let c: Vec<&str> = cmd.split_whitespace().collect();

            // don't continue if not enough parameters are available
            if c.len() < 3 {
                println!("Please define a valid light command and zone, like the following: ");
                println!("light on 2");
                continue;
            }

            // interprete the value to set
            let val;
            if let Ok(z) = f32::from_str(c[1]) {
                val = z;
            } else if c[1] == "off" {
                val = 0.0;
            } else {
                val = 1.0;
            }

            // interpret the group to set
            let group = c.get(3).and_then(|g| usize::from_str(g).ok());

            // when the zone can be converted to a number, set the value
            if let Ok(zone) = usize::from_str(c[2]) {
                appt.set_value(zone, group, dss::Value::Light(val))?;
            } else {
                println!("Please define a valid light command and zone, like the following: ");
                println!("light on 2");
            }
        }
        // set the shadow
        else if cmd.starts_with("shadow") {
            // split the command at white spaces
            let c: Vec<&str> = cmd.split_whitespace().collect();

            // don't continue if not enough parameters are available
            if c.len() < 4 {
                println!("Please define a valid shadow command and zone, like the following: ");
                println!("shadow open open 2");
                continue;
            }

            // interprete the open value to set
            let open;
            if let Ok(z) = f32::from_str(c[1]) {
                open = z;
            } else if c[1] == "close" {
                open = 1.0;
            } else {
                open = 0.0;
            }

            // interprete the angle value to set
            let angle;
            if let Ok(z) = f32::from_str(c[1]) {
                angle = z;
            } else if c[2] == "open" {
                angle = 1.0;
            } else {
                angle = 0.0;
            }

            // interpret the group to set
            let group = c.get(4).and_then(|g| usize::from_str(g).ok());

            // when the zone can be converted to a number, set the value
            if let Ok(zone) = usize::from_str(c[3]) {
                appt.set_value(zone, group, dss::Value::Shadow(open, angle))?;
            } else {
                println!("Please define a valid shadow command and zone, like the following: ");
                println!("shadow open open 2");
            }
        }
        // exit the programm
        else if cmd == "exit" {
            std::process::exit(0);
        // Show the help
        } else {
            println!("zones     Get all zones and the included data");
            println!("zone      Get a specific zone data");
            println!("light     Set the light for a zone");
            println!("shadow    Set the shadow for a zone");
            println!("exit      Exit the DSS CLI");
            println!("help      Show this help text");
            println!("");
        }
    }

    Ok(())
}

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
    dss::Appartement::new(host.trim(), user.trim(), pass.trim())
}
