fn main() {
    // Connect to the digital strom server
    let  appt = dss::Appartement::connect("dss", "dssadmin", "dssadmin").unwrap();

    // turn the light in the zone 2 and group 0 on
    appt.set_value(2, 0, dss::Value::Light(1))?;
}