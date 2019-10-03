fn main() -> Result<()> {
    //let api = Api::new("dss", "dssadmin", "dssadmin")?;
    //let (events, status) = api.new_event_channel()?;
    //*status.lock().unwrap() = false;

    let events;
    {
    let appt = Appartement::new("dss", "dssadmin", "dssadmin")?;
    println!("{:#?}", appt.get_zones());
    events = appt.event_channel()?;
    }

    //api.call_scene(2, Type::Light, 0);

    loop {
        let res = events.recv();

        match res {
            Ok(v) => println!("{:#?}", v),
            Err(_) => {
                println!("Channel Closed");
                break;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct Appartement {
    inner: std::sync::Arc<std::sync::Mutex<InnerAppartement>>,
}

impl Appartement {
    pub fn new<S>(host: S, user: S, password: S) -> Result<Appartement>
    where
        S: Into<String>,
    {
        // create the Appartment with the inner values
        let appt = Appartement {
            inner: std::sync::Arc::new(std::sync::Mutex::new(InnerAppartement {
                api: Api::new(host, user, password)?,
                zones: vec![],
                thread: std::sync::Arc::new(std::sync::Mutex::new(false)),
            })),
        };

        // update the complete structure
        appt.inner.lock()?.update_structure()?;

        Ok(appt)
    }

    /// Returns an vector of all zones with their groups.
    ///
    /// Keep in mind, that the values are in a frozen state.
    /// If you want to stay informed about changes, use the
    /// 'event_channel()' function.
    pub fn get_zones(&self) -> Result<Vec<Zone>> {
        Ok(self.inner.lock()?.zones.clone())
    }

    /// Updates the complete appartment structure, this command can take some time
    /// to execute (more than 10 seconds).
    ///
    /// Use the 'get_zones()' function to get the actual structure with updates
    /// values for each group.
    pub fn update_all(&self) -> Result<Vec<Zone>> {
        self.inner.lock()?.update_structure()?;
        Ok(self.inner.lock()?.zones.clone())
    }

    /// Get the event channel for the appartment.
    ///
    /// When a channel is already open for this apparment, we close the open one
    /// and create a new one which gets returned.
    /// Therefore it's not recommended to call this function twice for one appartment.
    pub fn event_channel(&self) -> Result<std::sync::mpsc::Receiver<Event>> {
        self.inner.lock()?.get_event_channel()
    }
}

#[derive(Debug)]
struct InnerAppartement {
    api: Api,
    zones: Vec<Zone>,
    thread: std::sync::Arc<std::sync::Mutex<bool>>,
}

impl InnerAppartement {
    /// Get the event channel from the API.
    ///
    /// When a channel is already open for this apparment, we close the open one
    /// and create a new one we are returning.
    /// Therefore it's not recommended to call this function twice for one appartment instance.
    fn get_event_channel(&mut self) -> Result<std::sync::mpsc::Receiver<Event>> {
        // when there are threads already existing close them
        if *self.thread.lock()? == true {
            *self.thread.lock()? = false;
        }

        // request a new channel
        let (recv, status) = self.api.new_event_channel()?;
        self.thread = status;
        Ok(recv)
    }

    fn update_structure(&mut self) -> Result<()> {
        let devices = self.api.get_devices()?;
        let mut zones: Vec<Zone> = self
            .api
            .get_zones()?
            .into_iter()
            .filter(|z| z.id != 0 && z.id != 65534)
            .collect();

        for zone in &mut zones {
            // add all the groups
            for typ in &zone.types {
                // get all available scenes for this zone
                let scenes = self.api.get_scenes(zone.id, typ.clone())?;

                // convert the scenes to groups
                let mut scene_groups = Group::from_scenes(&scenes, zone.id, &typ);

                // the last called scene for this typ within a zone
                let lcs = self.api.get_last_called_scene(zone.id, typ.clone())?;

                // convert the last called scene to an action
                let action = Action::new(typ.clone(), lcs);

                // add the last called action for each scene group
                scene_groups
                    .iter_mut()
                    .for_each(|g| g.status = Value::from_action(action.clone(), g.id));

                // add the scene groups to the group array
                zone.groups.append(&mut scene_groups);
            }

            // for every group add the devices
            for group in &mut zone.groups {
                // loop over all devices
                // filtered down to light and shadow devices
                for device in devices.iter().filter(|d| {
                    d.device_type == DeviceType::Light || d.device_type == DeviceType::Shadow
                }) {
                    // if the device matches and the group is 0 (which means general all devices)
                    if group.id == 0
                        && device.zone_id == group.zone_id
                        && group.typ == device.button_type
                    {
                        group.devices.push(device.clone());
                    }
                    // when the devices matches, but the scene group is not 0 we need to check where to sort the device
                    else if device.zone_id == group.zone_id && group.typ == device.button_type {
                        // check the device mode for this scene within that zone
                        let _ = self
                            .api
                            .get_device_scene_mode(device.id.clone(), group.id)
                            .and_then(|dsm| {
                                // when the device cares about this scene group we add it
                                if !dsm.dont_care {
                                    group.devices.push(device.clone());
                                }
                                Ok(())
                            });
                    }
                }
            }
        }

        self.zones = zones;

        Ok(())
    }
}

impl Drop for InnerAppartement {
    fn drop(&mut self) {
        // if it fails we can't stop the threads
        #[allow(unused_must_use)]
        {
            self.thread.lock().map(|mut v| *v = false);
        }
    }
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

    pub fn new_event_channel(
        &self,
    ) -> Result<(
        std::sync::mpsc::Receiver<Event>,
        std::sync::Arc<std::sync::Mutex<bool>>,
    )> {
        // shareable boolean to stop threads
        let thread_status = std::sync::Arc::new(std::sync::Mutex::new(true));

        // subscribe to event
        self.plain_request(
            "event/subscribe",
            Some(vec![("name", "callScene"), ("subscriptionID", "911")]),
        )?;

        // create a channel to send data from one to the other thread
        let (send, recv) = std::sync::mpsc::channel();

        // this thread is just receiving data and directly reconnects
        let this = self.clone();
        let ts = thread_status.clone();
        std::thread::spawn(move || loop {
            // listen for events at the server
            let res = this.plain_request(
                "event/get",
                Some(vec![("timeout", "3000"), ("subscriptionID", "911")]),
            );

            // check if the thread should be ended
            if *ts.lock().unwrap() == false {
                break;
            }

            // we have no plan B when the sending fails
            #[allow(unused_must_use)]
            {
                // send the result to the next thread
                send.send(res);
            }
        });

        // create a channel to send teh event to thhe receiver
        let (inp, out) = std::sync::mpsc::channel();

        // this thread is processing the data and calls the event handler
        let this = self.clone();
        let ts = thread_status.clone();
        std::thread::spawn(move || loop {
            // we have no plan B when an error occours
            #[allow(unused_must_use)]
            {
                // receive from the channel and continue if no channel recv error occoured
                let res = recv.recv();

                // check if the thread should be ended
                if *ts.lock().unwrap() == false {
                    break;
                }

                res.and_then(|res| {
                    // continue when no reqwest (http) error occoured
                    res.and_then(|mut v| {
                        // extract the json into an event array
                        this.extract_events(&mut v).and_then(|es| {
                            // for each event call the event handler function
                            es.into_iter().for_each(|e| {
                                let _tmp = inp.send(e);
                            });
                            Ok(())
                        });
                        Ok(())
                    });
                    Ok(())
                });
            }
        });

        Ok((out, thread_status))
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

            event.group = Group::group_id_from_scene_id(event.scene);

            event.value = Value::from_action(event.action.clone(), event.group);

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

        // transform the data to the zones
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

    pub fn get_device_scene_mode<S>(&self, device: S, scene_id: usize) -> Result<SceneMode>
    where
        S: Into<String>,
    {
        let json = self.plain_request(
            "device/getSceneMode",
            Some(vec![
                ("dsid", &device.into()),
                ("sceneID", &scene_id.to_string()),
            ]),
        )?;

        // convert to SceneMode
        Ok(serde_json::from_value(json)?)
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

    #[serde(default)]
    pub value: Value,

    #[serde(default)]
    pub group: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Zone {
    #[serde(alias = "zoneID")]
    pub id: usize,
    pub name: String,
    #[serde(alias = "groups")]
    pub types: Vec<Type>,
    #[serde(default)]
    pub groups: Vec<Group>,
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

impl Action {
    fn new(typ: Type, scene: usize) -> Action {
        if typ == Type::Light && scene == 0 {
            return Action::AllLightOff;
        }

        if typ == Type::Light && scene == 5 {
            return Action::AllLightOn;
        }

        if typ == Type::Shadow && scene == 0 {
            return Action::AllShadowDown;
        }

        if typ == Type::Shadow && scene == 5 {
            return Action::AllShadowUp;
        }

        if scene > 0 && scene < 5 {
            if typ == Type::Light {
                return Action::LightOff(scene);
            } else if typ == Type::Shadow {
                return Action::ShadowDown(scene);
            }
        }

        if scene > 5 && scene < 9 {
            if typ == Type::Light {
                return Action::LightOn(scene - 5);
            } else if typ == Type::Shadow {
                return Action::ShadowUp(scene - 5);
            }
        }

        if typ == Type::Shadow && scene == 55 {
            return Action::AllShadowStop;
        }

        if typ == Type::Shadow && scene > 50 && scene < 55 {
            return Action::ShadowStop(scene - 51);
        }

        if typ == Type::Shadow && scene == 42 {
            return Action::ShadowStepClose;
        }

        if typ == Type::Shadow && scene == 43 {
            return Action::ShadowStepOpen;
        }

        if typ == Type::Shadow && scene == 17 {
            return Action::AllShadowUp;
        }

        if typ == Type::Shadow && scene == 18 {
            return Action::AllShadowSpecial1;
        }

        if typ == Type::Shadow && scene == 19 {
            return Action::AllShadowSpecial2;
        }

        Action::Unknown
    }
}

impl Default for Action {
    fn default() -> Self {
        Action::Unknown
    }
}

impl From<Event> for Action {
    fn from(e: Event) -> Self {
        Action::new(e.typ, e.scene)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum Value {
    Light(f32),
    Shadow(f32, f32),
    Unknown,
}

impl Value {
    pub fn from_action(action: Action, _id: usize) -> Self {
        match action {
            Action::AllLightOn => Value::Light(1.0),
            Action::LightOn(_id) => Value::Light(1.0),
            Action::AllLightOff => Value::Light(0.0),
            Action::LightOff(_id) => Value::Light(0.0),
            Action::AllShadowUp => Value::Shadow(0.0, 1.0),
            Action::AllShadowDown => Value::Shadow(1.0, 0.0),
            Action::ShadowDown(_id) => Value::Shadow(1.0, 0.0),
            Action::ShadowUp(_id) => Value::Shadow(0.0, 1.0),
            _ => Value::Unknown,
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Unknown
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
    #[serde(alias = "buttonActiveGroup")]
    pub button_type: Type,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SceneMode {
    #[serde(alias = "sceneID")]
    pub scene: usize,
    #[serde(alias = "dontCare")]
    pub dont_care: bool,
    #[serde(alias = "localPrio")]
    pub local_prio: bool,
    #[serde(alias = "specialMode")]
    pub special_mode: bool,
    #[serde(alias = "flashMode")]
    pub flash_mode: bool,
    #[serde(alias = "ledconIndex")]
    pub led_con_index: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Group {
    id: usize,
    zone_id: usize,
    typ: Type,
    status: Value,
    devices: Vec<Device>,
}

impl Group {
    pub fn new(id: usize, zone_id: usize, typ: Type) -> Self {
        Group {
            id,
            zone_id: zone_id,
            typ,
            devices: vec![],
            status: Value::default(),
        }
    }

    pub fn group_id_from_scene_id(scene: usize) -> usize {
        if scene > 0 && scene < 5 {
            return scene;
        }

        if scene > 5 && scene < 9 {
            return scene - 5;
        }

        if scene > 50 && scene < 55 {
            return scene - 51;
        }

        0
    }

    pub fn from_scene(scene: usize, zone_id: usize, typ: &Type) -> Option<Group> {
        // add the different scene groups if they exist
        for x in 1..4 {
            if scene == x {
                return Some(Group::new(x, zone_id, typ.clone()));
            }
        }

        // if no different scene groups availabe, we take the general one
        if scene == 0 {
            return Some(Group::new(0, zone_id, typ.clone()));
        }

        None
    }

    pub fn from_scenes(scenes: &[usize], zone_id: usize, typ: &Type) -> Vec<Group> {
        scenes
            .iter()
            .filter_map(|s| Group::from_scene(*s, zone_id, typ))
            .collect()
    }
}

impl Default for Group {
    fn default() -> Self {
        Group {
            id: 0,
            zone_id: 0,
            typ: Type::Unknown,
            devices: vec![],
            status: Value::default(),
        }
    }
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

// implement mutex poison error
impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(_err: std::sync::PoisonError<T>) -> Self {
        Error::from("Poison error")
    }
}
