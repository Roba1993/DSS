fn main() {
    let api = Api::new("dss", "dssadmin", "dssadmin").unwrap();

    api.set_event_handler(&|e| println!("{:?}", e));


    println!("{:?}", api.get_zones());
    println!("{:?}", api.get_scenes(1, Type::Shadow));
    println!("{:?}", api.get_scenes(2, Type::Shadow));
    println!("{:?}", api.get_scenes(10, Type::Light));

    println!("{:#?}", api.get_devices());

    // let res = api.get_shadow_device_angle("303505d7f8000f800009a711");
    // let res = api.set_shadow_device_angle("303505d7f8000f800009a711", 0.5);
    // println!("{:?}", res);

    std::thread::sleep_ms(600000);
}

#[derive(Debug, Clone)]
pub struct Api {
    host: String,
    user: String,
    password: String,
    token: String,
}

impl Api {
    pub fn new<S>(host: S, user: S, password: S) -> Result<Api>
    where
        S: Into<String>,
    {
        let mut api = Api {
            host: host.into(),
            user: user.into(),
            password: password.into(),
            token: String::from(""),
        };

        api.login()?;

        Ok(api)
    }

    pub fn login(&mut self) -> Result<()> {
        // build the client and allow invalid certificates
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()?;

        // make the login request
        let mut response = client
            .get(&format!("https://{}:8080/json/system/login", self.host))
            .query(&[("user", &self.user), ("password", &self.password)])
            .send()?;

        // get the result as Json Value
        let json: serde_json::Value = response.json()?;

        // extract the token
        self.token = json
            .get("result")
            .ok_or("No result in Json response")?
            .get("token")
            .ok_or("No token in Json response")?
            .as_str()
            .ok_or("Token is not a String")?
            .to_string();

        Ok(())
    }

    pub fn plain_request<S>(
        &self,
        request: S,
        parameter: Option<Vec<(&str, &str)>>,
    ) -> Result<serde_json::Value>
    where
        S: Into<String>,
    {
        // Handle parameter and add token
        let parameter = match parameter {
            None => vec![("token", self.token.as_str())],
            Some(mut p) => {
                p.push(("token", &self.token));
                p
            }
        };

        // build the client and allow invalid certificates
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(None)
            .build()?;

        // make the login request
        let mut response = client
            .get(&format!(
                "https://{}:8080/json/{}",
                self.host,
                request.into()
            ))
            .query(&parameter)
            .send()?;

        let mut json: serde_json::Value = response.json()?;

        // check if the response was sucessfull
        if !json
            .get("ok")
            .ok_or("No ok in Json response")?
            .as_bool()
            .ok_or("No boolean ok code")?
        {
            return Err("Request failed, no ok code received".into());
        }

        // take the result and return it
        match json.get_mut("result") {
            None => Ok(serde_json::json!(null)),
            Some(j) => Ok(j.take()),
        }
    }

    pub fn set_event_handler<F>(&self, func: &'static F)
    where
        F: Fn(Event) + std::marker::Sync,
    {
        // create a channel to send dataa from one to the other thread
        let (send, recv) = std::sync::mpsc::channel();

        // this thread is just receiving data and directly reconnects
        let this = self.clone();
        std::thread::spawn(move || loop {
            // listen for events at the server
            let res = this.plain_request(
                "event/get",
                Some(vec![("timeout", "3000"), ("subscriptionID", "911")]),
            );

            // we have no plan B when the sending fails
            #[allow(unused_must_use)]
            {
                // send the result to the next thread
                send.send(res);
            }
        });

        // this thread is processing the data and calls the event handler
        let this = self.clone();
        std::thread::spawn(move || loop {
            // we have no plan B when an error occours
            #[allow(unused_must_use)]
            {
                // receive from the channel and continue if no channel recv error occoured
                recv.recv().and_then(|res| {
                    // continue when no reqwest (http) error occoured
                    res.and_then(|mut v| {
                        // extract the json into an event array
                        this.extract_events(&mut v).and_then(|es| {
                            // for each event call the event handler function
                            es.into_iter().for_each(|e| func(e));
                            Ok(())
                        });
                        Ok(())
                    });
                    Ok(())
                });
            }

        });
    }

    fn extract_events(&self, json: &mut serde_json::Value) -> Result<Vec<Event>> {
        let events = json
            .get_mut("events")
            .ok_or("No events available")?
            .as_array_mut()
            .take()
            .ok_or("Events not in array")?;

        let mut out = vec![];

        for e in events {
            let name = e
                .get("name")
                .ok_or("No name for event")?
                .as_str()
                .ok_or("Event name not a string")?
                .to_string();
            let props = e
                .get_mut("properties")
                .ok_or("No properties in event")?
                .take();

            let mut event: Event = serde_json::from_value(props)?;
            event.name = name;

            event.action = Action::from(event.clone());

            out.push(event);
        }

        Ok(out)
    }

    pub fn get_apartment_name(&self) -> Result<String> {
        // extract the name
        Ok(self
            .plain_request("apartment/getName", None)?
            .get("result")
            .ok_or("No result in Json response")?
            .get("name")
            .ok_or("No name in Json response")?
            .as_str()
            .ok_or("Name is not a String")?
            .to_string())
    }

    pub fn set_apartment_name<S>(&self, new_name: S) -> Result<bool>
    where
        S: Into<String>,
    {
        // extract the name
        Ok(self
            .plain_request(
                "apartment/getName",
                Some(vec![("newName", &new_name.into())]),
            )?
            .get("ok")
            .ok_or("No ok in Json response")?
            .as_bool()
            .ok_or("No boolean ok code")?)
    }

    pub fn get_zones(&self) -> Result<Vec<Zone>> {
        let mut json = self.plain_request("apartment/getReachableGroups", None)?;

        // unpack the zones
        let json = json
            .get_mut("zones")
            .ok_or("No zones in Json response")?
            .take();

        // transform the date to the zones
        Ok(serde_json::from_value(json)?)
    }

    pub fn get_zone_name(&self, id: usize) -> Result<String> {
        let res = self.plain_request("zone/getName", Some(vec![("id", &id.to_string())]))?;

        // unpack the name
        let name = res
            .get("name")
            .ok_or("No name returned")?
            .as_str()
            .ok_or("No String value available")?;

        Ok(name.to_string())
    }

    pub fn get_devices(&self) -> Result<Vec<Device>> {
        let res = self.plain_request("apartment/getDevices", None)?;

        Ok(serde_json::from_value(res)?)
    }

    pub fn get_circuits(&self) -> Result<Vec<Circut>> {
        let mut res = self.plain_request("apartment/getCircuits", None)?;

        let res = res
            .get_mut("circuits")
            .ok_or("No circuits available")?
            .take();

        Ok(serde_json::from_value(res)?)
    }

    pub fn get_scenes(&self, zone: usize, typ: Type) -> Result<Vec<usize>> {
        // convert the enum to usize
        let typ = typ as usize;

        let mut json = self.plain_request(
            "zone/getReachableScenes",
            Some(vec![
                ("id", &zone.to_string()),
                ("groupID", &typ.to_string()),
            ]),
        )?;

        // unpack the scenes
        let json = json
            .get_mut("reachableScenes")
            .ok_or("No scenes returned")?
            .take();

        // convert to number array
        Ok(serde_json::from_value(json)?)
    }

    pub fn get_last_called_scene(&self, zone: usize, typ: Type) -> Result<usize> {
        // convert the enum to usize
        let typ = typ as usize;

        let res = self.plain_request(
            "zone/getLastCalledScene",
            Some(vec![
                ("id", &zone.to_string()),
                ("groupID", &typ.to_string()),
            ]),
        )?;

        // unpack the scene
        let number = res
            .get("scene")
            .ok_or("No scene returned")?
            .as_u64()
            .ok_or("No scene number available")?;

        Ok(number as usize)
    }

    pub fn call_scene(&self, zone: usize, typ: Type, scene: usize) -> Result<()> {
        // convert the enum to usize
        let typ = typ as usize;

        self.plain_request(
            "zone/callScene",
            Some(vec![
                ("id", &zone.to_string()),
                ("groupID", &typ.to_string()),
                ("sceneNumber", &scene.to_string()),
            ]),
        )?;

        Ok(())
    }


    pub fn get_shadow_device_open<S>(&self, device: S) -> Result<f32>
    where
        S: Into<String>,
    {
        // make the request
        let res = self.plain_request(
            "device/getOutputValue",
            Some(vec![("dsid", &device.into()), ("offset", "2")]),
        )?;

        // check for the right offset
        if res
            .get("offset")
            .ok_or("No offset returnes")?
            .as_u64()
            .ok_or("The offset is not a number")?
            != 2
        {
            return Err(Error::from("Wrong offset returned"));
        }

        // extract the value
        let value = res
            .get("value")
            .ok_or("No value returnes")?
            .as_u64()
            .ok_or("The value is not a number")?;

        // get the procentage
        let value = (value as f32) / 65535.0;

        // turn the value around
        Ok(1.0 - value)
    }


    pub fn set_shadow_device_open<S>(&self, device: S, value: f32) -> Result<()>
    where
        S: Into<String>,
    {
        // min size is 0
        let value = value.max(0.0);

        // max size is 1
        let value = value.min(1.0);

        // move the direction 1 is down 0 is up
        let value = 1.0 - value;

        // transform to dss range
        let value = (65535.0 * value) as usize;

        // make the request
        self.plain_request(
            "device/setOutputValue",
            Some(vec![
                ("dsid", &device.into()),
                ("value", &format!("{}", value)),
                ("offset", "2"),
            ]),
        )?;

        Ok(())
    }

    pub fn get_shadow_device_angle<S>(&self, device: S) -> Result<f32>
    where
        S: Into<String>,
    {
        // make the request
        let res = self.plain_request(
            "device/getOutputValue",
            Some(vec![("dsid", &device.into()), ("offset", "4")]),
        )?;

        // check for the right offset
        if res
            .get("offset")
            .ok_or("No offset returnes")?
            .as_u64()
            .ok_or("The offset is not a number")?
            != 4
        {
            return Err(Error::from("Wrong offset returned"));
        }

        // extract the value
        let value = res
            .get("value")
            .ok_or("No value returnes")?
            .as_u64()
            .ok_or("The value is not a number")?;

        // get the procentage
        let value = (value as f32) / 65535.0;

        Ok(value)
    }

    pub fn set_shadow_device_angle<S>(&self, device: S, value: f32) -> Result<()>
    where
        S: Into<String>,
    {
        // min size is 0
        let value = value.max(0.0);

        // max size is 1
        let value = value.min(1.0);

        // transform to dss range
        let value = (255.0 * value) as usize;

        // make the request
        self.plain_request(
            "device/setOutputValue",
            Some(vec![
                ("dsid", &device.into()),
                ("value", &format!("{}", value)),
                ("offset", "4"),
            ]),
        )?;

        Ok(())
    }
}

fn from_str<'de, T, D>(deserializer: D) -> std::result::Result<T, D::Error>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    let s = String::deserialize(deserializer)?;
    T::from_str(&s).map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Event {
    #[serde(default)]
    pub name: String,

    #[serde(alias = "zoneID", deserialize_with = "from_str")]
    pub zone: usize,

    #[serde(alias = "groupID", deserialize_with = "from_str")]
    pub typ: Type,

    #[serde(alias = "sceneID", deserialize_with = "from_str")]
    pub scene: usize,

    #[serde(alias = "originToken")]
    pub token: String,

    #[serde(alias = "originDSUID")]
    pub dsuid: String,

    #[serde(alias = "callOrigin")]
    pub origin: String,

    #[serde(default)]
    pub action: Action,
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Zone {
    #[serde(alias = "zoneID")]
    pub id: usize,
    pub name: String,
    #[serde(alias = "groups")]
    pub types: Vec<Type>,
}

#[derive(serde_repr::Serialize_repr, serde_repr::Deserialize_repr, PartialEq, Debug, Clone)]
#[repr(u8)]
pub enum Type {
    Unknown = 0,
    Light = 1,
    Shadow = 2,
    Heating = 3,
    Audio = 4,
    Video = 5,
    Joker = 8,
    Cooling = 9,
    Ventilation = 10,
    Window = 11,
    AirRecirculation = 12,
    TemperatureControl = 48,
    ApartmentVentilation = 64,
}

impl From<u8> for Type {
    fn from(u: u8) -> Self {
        match u {
            1 => Type::Light,
            2 => Type::Shadow,
            3 => Type::Heating,
            4 => Type::Audio,
            5 => Type::Video,
            8 => Type::Joker,
            9 => Type::Cooling,
            10 => Type::Ventilation,
            11 => Type::Window,
            12 => Type::AirRecirculation,
            48 => Type::TemperatureControl,
            64 => Type::ApartmentVentilation,
            _ => Type::Unknown,
        }
    }
}

impl std::str::FromStr for Type {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let u = u8::from_str(s)?;
        Ok(Type::from(u))
    }
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub enum Action {
    AllLightOn,
    AllLightOff,
    LightOn(usize),
    LightOff(usize),
    AllShadowUp,
    AllShadowDown,
    ShadowUp(usize),
    ShadowDown(usize),
    AllShadowStop,
    ShadowStop(usize),
    ShadowStepOpen,
    ShadowStepClose,
    AllShadowSpecial1,
    AllShadowSpecial2,
    Unknown,
}

impl Default for Action {
    fn default() -> Self {
        Action::Unknown
    }
}

impl From<Event> for Action {
    fn from(e: Event) -> Self {
        if e.typ == Type::Light && e.scene == 0 {
            return Action::AllLightOff;
        }

        if e.typ == Type::Light && e.scene == 5 {
            return Action::AllLightOn;
        }

        if e.typ == Type::Shadow && e.scene == 0 {
            return Action::AllShadowDown;
        }

        if e.typ == Type::Shadow && e.scene == 5 {
            return Action::AllShadowUp;
        }

        if e.scene > 0 && e.scene < 5 {
            if e.typ == Type::Light {
                return Action::LightOff(e.scene - 1);
            } else if e.typ == Type::Shadow {
                return Action::ShadowDown(e.scene - 1);
            }
        }

        if e.scene > 4 && e.scene < 9 {
            if e.typ == Type::Light {
                return Action::LightOn(e.scene - 5);
            } else if e.typ == Type::Shadow {
                return Action::ShadowUp(e.scene - 5);
            }
        }

        if e.typ == Type::Shadow && e.scene == 55 {
            return Action::AllShadowStop;
        }

        if e.typ == Type::Shadow && e.scene > 50 && e.scene < 55 {
            return Action::ShadowStop(e.scene - 51);
        }

        if e.typ == Type::Shadow && e.scene == 42 {
            return Action::ShadowStepClose;
        }

        if e.typ == Type::Shadow && e.scene == 43 {
            return Action::ShadowStepOpen;
        }

        if e.typ == Type::Shadow && e.scene == 17 {
            return Action::AllShadowUp;
        }

        if e.typ == Type::Shadow && e.scene == 18 {
            return Action::AllShadowSpecial1;
        }

        if e.typ == Type::Shadow && e.scene == 19 {
            return Action::AllShadowSpecial2;
        }

        Action::Unknown
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    #[serde(alias = "zoneID")]
    pub zone_id: usize,
    #[serde(alias = "isPresent")]
    pub present: bool,
    #[serde(alias = "outputMode")]
    pub device_type: DeviceType,
    #[serde(alias = "groups")]
    pub types: Vec<Type>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum DeviceType {
    Switch,
    Light,
    Tv,
    Shadow,
    Unknown,
}

impl From<usize> for DeviceType {
    fn from(num: usize) -> Self {
        match num {
            0 => DeviceType::Switch,
            16 | 22 | 35 => DeviceType::Light,
            33 => DeviceType::Shadow,
            39 => DeviceType::Tv,
            _ => DeviceType::Unknown,
        }
    }
}

impl<'de> serde::Deserialize<'de> for DeviceType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<DeviceType, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(DeviceType::from(usize::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Circut {
    #[serde(alias = "dsid")]
    pub id: String,
    pub name: String,
    #[serde(alias = "isPresent")]
    pub present: bool,
    #[serde(alias = "isValid")]
    pub valid: bool,

}


pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Error(String),
    SerdeJson(serde_json::Error),
    Reqwest(reqwest::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Error(s) => write!(f, "{}", s),
            Error::SerdeJson(ref e) => e.fmt(f),
            Error::Reqwest(ref e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        match self {
            Error::Error(s) => &s,
            Error::SerdeJson(ref e) => e.description(),
            Error::Reqwest(ref e) => e.description(),
        }
    }

    fn cause(&self) -> Option<&std::error::Error> {
        match self {
            Error::Error(_) => None,
            Error::SerdeJson(ref e) => Some(e),
            Error::Reqwest(ref e) => Some(e),
        }
    }
}

// immplement error from string
impl From<&str> for Error {
    fn from(err: &str) -> Self {
        Error::Error(err.into())
    }
}

// immplement Serde Json
impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::SerdeJson(err)
    }
}

// immplement Serde Json
impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Reqwest(err)
    }
}