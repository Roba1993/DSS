/// The appartment gives you a easy and highlevel interface
/// to a dss installation. It's the main struct this crates
/// provides.
///
/// Please only use this interaface to a dss installation and
/// not any other interface to the same installation at the same time.
/// This crate is buffering information to keep the dss actions
/// reponse times somewhat decent.
/// Also don't use the RawAPI directly, because it has the same effect as
/// using another interface and is leading to mismathcing data.
/// This is unfortunatelly due to the really bad designed API from DSS.
#[derive(Debug, Clone)]
pub struct Appartement {
    inner: std::sync::Arc<std::sync::Mutex<InnerAppartement>>,
}

impl Appartement {
    /// Connect to a DSS installation and fetch the complete structure of it.
    /// Based on your appartment size, this can take around a minute.
    pub fn connect<S>(host: S, user: S, password: S) -> Result<Appartement>
    where
        S: Into<String>,
    {
        // create the Appartment with the inner values
        let appt = Appartement {
            inner: std::sync::Arc::new(std::sync::Mutex::new(InnerAppartement {
                api: RawApi::connect(host, user, password)?,
                zones: vec![],
                file: None,
                thread: std::sync::Arc::new(std::sync::Mutex::new(false)),
            })),
        };

        // update the complete structure
        appt.inner.lock()?.update_structure()?;

        Ok(appt)
    }

    /// Connect to a DSS installation and load structure from file
    pub fn connect_file<S>(host: S, user: S, password: S, file: S) -> Result<Appartement>
    where
        S: Into<String>,
    {
        let file = file.into();

        // load the zones from file
        let mut zones = vec![];
        if let Ok(s) = std::fs::read_to_string(&file) {
            let r = serde_json::from_str(&s);
            if let Ok(z) = r {
                zones = z;
            } else {
                println!("{:?}", r);
            }
        }

        // create the Appartment with the inner values
        let appt = Appartement {
            inner: std::sync::Arc::new(std::sync::Mutex::new(InnerAppartement {
                api: RawApi::connect(host, user, password)?,
                zones: zones,
                file: Some(file),
                thread: std::sync::Arc::new(std::sync::Mutex::new(false)),
            })),
        };

        // update the complete structure if no zones where loaded
        {
            let mut a = appt.inner.lock()?;
            if a.zones.is_empty() {
                a.update_structure()?;
            }
        }

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
    /// to execute (more than 30 seconds).
    ///
    /// Use the 'get_zones()' function to get the actual structure with updates
    /// values for each group.
    pub fn update_all(&self) -> Result<Vec<Zone>> {
        self.inner.lock()?.update_structure()?;
        Ok(self.inner.lock()?.zones.clone())
    }

    pub fn get_value(&self, zone: usize, group: usize) -> Result<Value> {
        self.inner.lock()?.get_value(zone, group)
    }

    pub fn set_value(&self, zone: usize, group: Option<usize>, value: Value) -> Result<()> {
        self.inner.lock()?.set_value(zone, group, value)
    }

    /// Get the event channel for the appartment.
    ///
    /// When a channel is already open for this apparment, we close the open one
    /// and create a new one which gets returned.
    /// Therefore it's not recommended to call this function twice for one appartment.
    pub fn event_channel(&self) -> Result<std::sync::mpsc::Receiver<Event>> {
        // when there are threads already existing close them
        if *self.inner.lock()?.thread.lock()? == true {
            *self.inner.lock()?.thread.lock()? = false;
        }

        // request a new event channel
        let (recv, status) = self.inner.lock()?.api.new_event_channel()?;

        // create the new out channel
        let (inp, out) = std::sync::mpsc::channel();

        // clone the status for the thread & the structure
        let internal_status = status.clone();
        let appr = self.inner.clone();

        std::thread::spawn(move || loop {
            // listen for events to pop up
            let event = match recv.recv() {
                Ok(e) => e,
                Err(_) => break,
            };

            // check if the thread should be ended
            if *internal_status.lock().unwrap() == false {
                break;
            }

            // expand the events when necessary
            let events = appr.lock().unwrap().expand_value(event).unwrap();

            for event in events {
                // update the event value for shadow etc.
                let event = appr.lock().unwrap().update_event_value(event).unwrap();

                // update the appartment structure with the event
                {
                    let mut appr = appr.lock().unwrap();
                    appr.zones.iter_mut().for_each(|z| {
                        // fine the right zone to the event
                        if z.id == event.zone {
                            z.groups.iter_mut().for_each(|g| {
                                // find the right group typ && id to update the value
                                if g.typ == event.typ && g.id == event.group {
                                    g.status = event.value.clone();
                                }
                            });
                        }
                    });

                    if let Err(e) = appr.save_status() {
                        println!("Error while saving: {}", e);
                    }
                }

                // send the event
                match inp.send(event) {
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        // update the thread status with the new one received
        self.inner.lock()?.thread = status;
        Ok(out)
    }
}

#[derive(Debug)]
struct InnerAppartement {
    api: RawApi,
    zones: Vec<Zone>,
    file: Option<String>,
    thread: std::sync::Arc<std::sync::Mutex<bool>>,
}

impl InnerAppartement {
    fn get_value(&self, zone: usize, group: usize) -> Result<Value> {
        self.zones
            .iter()
            .find(|z| z.id == zone)
            .and_then(|z| z.groups.iter().find(|g| g.id == group))
            .map(|g| g.status.clone())
            .ok_or("No Value found for given device".into())
    }

    fn set_value(&mut self, zone: usize, group: Option<usize>, value: Value) -> Result<()> {
        // when a group exist we control the special group
        if let Some(grp) = group {
            match value {
                // depending on the value we turn the light on or off
                Value::Light(light) => {
                    if light < 0.5 {
                        self.api.call_action(zone, Action::LightOff(grp))?;
                    } else {
                        self.api.call_action(zone, Action::LightOn(grp))?;
                    }
                }
                // actions need to be performed for setting the shadow
                Value::Shadow(open, angle) => {
                    if open <= 0.1 {
                        self.api.call_action(zone, Action::ShadowUp(grp))?;
                    }
                    if open >= 0.9 && angle <= 0.1 {
                        self.api.call_action(zone, Action::ShadowDown(grp))?;
                    } else {
                        // we need to set a specific position and angle, this can't be done over an scene
                        // we need to get all devices for the actual zone and show type
                        let devices = self
                            .zones
                            .iter()
                            .find(|z| z.id == zone)
                            .ok_or("Not a valid zone id given")?
                            .groups
                            .iter()
                            .find(|g| g.id == grp)
                            .ok_or("Not a valid group given")?
                            .devices
                            .iter()
                            .filter(|d| d.device_type == DeviceType::Shadow)
                            .clone();

                        for dev in devices {
                            self.api.set_shadow_device_open(dev.id.clone(), open)?;
                            self.api.set_shadow_device_angle(dev.id.clone(), angle)?;
                        }

                        // we need to update the state of the shadow manually, because no event will be triggered
                        self.zones
                            .iter_mut()
                            .find(|z| z.id == zone)
                            .ok_or("No valid zone given")?
                            .groups
                            .iter_mut()
                            .find(|g| g.id == grp)
                            .ok_or("Not a valid group given")?
                            .status = value;
                    }
                }
                Value::Unknown => (),
            }
        }
        // when no group is defined we controll the whole zone
        else {
            match value {
                // depending on the value we turn the light on or off
                Value::Light(light) => {
                    if light < 0.5 {
                        self.api.call_action(zone, Action::AllLightOff)?;
                    } else {
                        self.api.call_action(zone, Action::AllLightOn)?;
                    }
                }
                // actions need to be performed for setting the shadow
                Value::Shadow(open, angle) => {
                    if open <= 0.1 {
                        self.api.call_action(zone, Action::AllShadowUp)?;
                    }
                    if open >= 0.9 && angle <= 0.1 {
                        self.api.call_action(zone, Action::AllShadowDown)?;
                    } else {
                        // we need to set a specific position and angle, this can't be done over an scene

                        // we need to get all devices for the actual zone
                        let devices = self
                            .zones
                            .iter()
                            .find(|z| z.id == zone)
                            .ok_or("Not a valid zone id given")?
                            .groups
                            .iter()
                            .map(|g| g.devices.clone())
                            .flatten()
                            .collect::<Vec<Device>>()
                            .into_iter()
                            .filter(|d| d.device_type == DeviceType::Shadow);

                        for dev in devices {
                            self.api.set_shadow_device_open(dev.id.clone(), open)?;
                            self.api.set_shadow_device_angle(dev.id.clone(), angle)?;
                        }

                        // we need to update the state of the shadow manually, because no event will be triggered
                        self.zones
                            .iter_mut()
                            .find(|z| z.id == zone)
                            .ok_or("Not a valid zone id given")?
                            .groups
                            .iter_mut()
                            .filter(|g| g.typ == Type::Shadow)
                            .for_each(|g| g.status = value.clone());
                    }
                }
                Value::Unknown => (),
            }
        }

        Ok(())
    }

    fn expand_value(&self, event: Event) -> Result<Vec<Event>> {
        // when we have an action of type ShadowStepOpen
        // it effects all groups of a zone and we create multiple events for it
        // if we have multiple groups for this typ
        if event.typ == Type::Shadow
            && (event.action == Action::ShadowStepOpen || event.action == Action::ShadowStepClose)
        {
            // we get all group id's with Shadow within the event zone
            let groups: Vec<usize> = self
                .zones
                .iter()
                .find(|z| z.id == event.zone)
                .ok_or("No matching zone available")?
                .groups
                .iter()
                .filter(|g| g.typ == Type::Shadow)
                .map(|g| g.id)
                .collect();

            // for each shadow group we create a new event
            return Ok(groups
                .iter()
                .map(|g| {
                    let mut e = event.clone();
                    e.group = *g;
                    e
                })
                .collect());
        }

        Ok(vec![event])
    }

    fn update_event_value(&self, event: Event) -> Result<Event> {
        // make the event mutable
        let mut event = event;

        // update the value
        event.value = self.update_value(event.value, &event.typ, event.zone, event.group)?;
        Ok(event)
    }

    fn update_value(&self, value: Value, typ: &Type, zone: usize, group: usize) -> Result<Value> {
        // when the value is already defined, the event is already updated
        if value != Value::Unknown {
            return Ok(value);
        }

        // let's fix the shadow events
        if typ == &Type::Shadow {
            // get the first device for the defined group
            let device = self
                .zones
                .iter()
                .find(|z| z.id == zone)
                .ok_or("No matching zone found")?
                .groups
                .iter()
                .find(|g| g.id == group && g.typ == Type::Shadow)
                .ok_or("No matching group found")?
                .devices
                .get(0)
                .ok_or("No devices available")?;

            // get the actual device values
            let open = self.api.get_shadow_device_open(&device.id)?;
            let angle = self.api.get_shadow_device_angle(&device.id)?;

            // return them in the Shadow format
            return Ok(Value::Shadow(open, angle));
        }

        Ok(value)
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

                // the last called action for shadows are always wrong,
                // so we set it directly to unknown and dont request the last called scene
                let action;
                if typ == &Type::Shadow {
                    action = Action::Unknown;
                } else {
                    // get the last called scene for this typ within a zone
                    let lcs = self.api.get_last_called_scene(zone.id, typ.clone())?;
                    // convert the last called scene to an action
                    action = Action::new(typ.clone(), lcs);
                }

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

        self.zones = zones.clone();

        for zone in &mut zones {
            // for every shadow group get the shadow values
            for group in zone.groups.iter_mut().filter(|g| g.typ == Type::Shadow) {
                // get the real shadow value
                let status = self.update_value(group.status.clone(), &group.typ, zone.id, group.id);

                // when the value available, then set it
                match status {
                    Ok(v) => group.status = v,
                    Err(_) => continue,
                }
            }
        }

        self.zones = zones;
        if let Err(e) = self.save_status() {
            println!("Error while saving: {}", e);
        }

        Ok(())
    }

    fn save_status(&self) -> Result<()> {
        if let Some(file) = &self.file {
            let content = serde_json::to_string(&self.zones)?;
            std::fs::write(file, content)?;
        }
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

/// Raw interface towards the DSS-Rest service. This is not intend to be used
/// directly from a API consumer. It misses important status management and
/// abstraction over the different devices.
#[derive(Debug, Clone)]
pub struct RawApi {
    host: String,
    user: String,
    password: String,
    token: String,
}

impl RawApi {
    /// Connect to the Digital Strom Server and try to login.
    pub fn connect<S>(host: S, user: S, password: S) -> Result<Self>
    where
        S: Into<String>,
    {
        let mut api = RawApi {
            host: host.into(),
            user: user.into(),
            password: password.into(),
            token: String::from(""),
        };

        api.login()?;

        Ok(api)
    }

    fn login(&mut self) -> Result<()> {
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

    /// Generic requset function, which handles the token inserting/login,
    /// the json parsing and success check.
    ///
    /// It returns a json value, dependet on the request.
    pub fn generic_request<S>(
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

    /// Create a new event channel, which is listinging to events from the dss station.
    pub fn new_event_channel(
        &self,
    ) -> Result<(
        std::sync::mpsc::Receiver<Event>,
        std::sync::Arc<std::sync::Mutex<bool>>,
    )> {
        // shareable boolean to stop threads
        let thread_status = std::sync::Arc::new(std::sync::Mutex::new(true));

        // subscribe to event
        self.generic_request(
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
            let res = this.generic_request(
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

        // create a channel to send the event to thhe receiver
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

    /// Receive the appartement name.
    pub fn get_appartement_name(&self) -> Result<String> {
        // extract the name
        Ok(self
            .generic_request("apartment/getName", None)?
            .get("result")
            .ok_or("No result in Json response")?
            .get("name")
            .ok_or("No name in Json response")?
            .as_str()
            .ok_or("Name is not a String")?
            .to_string())
    }

    /// Set the appartement name within the DSS.
    pub fn set_appartement_name<S>(&self, new_name: S) -> Result<bool>
    where
        S: Into<String>,
    {
        // extract the name
        Ok(self
            .generic_request(
                "apartment/getName",
                Some(vec![("newName", &new_name.into())]),
            )?
            .get("ok")
            .ok_or("No ok in Json response")?
            .as_bool()
            .ok_or("No boolean ok code")?)
    }

    /// Request all zones from the DSS system.
    pub fn get_zones(&self) -> Result<Vec<Zone>> {
        let mut json = self.generic_request("apartment/getReachableGroups", None)?;

        // unpack the zones
        let json = json
            .get_mut("zones")
            .ok_or("No zones in Json response")?
            .take();

        // transform the data to the zones
        Ok(serde_json::from_value(json)?)
    }

    /// Get the name of a specific zone from the DSS system.
    pub fn get_zone_name(&self, id: usize) -> Result<String> {
        let res = self.generic_request("zone/getName", Some(vec![("id", &id.to_string())]))?;

        // unpack the name
        let name = res
            .get("name")
            .ok_or("No name returned")?
            .as_str()
            .ok_or("No String value available")?;

        Ok(name.to_string())
    }

    /// Receive all devices availble in the appartement.
    pub fn get_devices(&self) -> Result<Vec<Device>> {
        let res = self.generic_request("apartment/getDevices", None)?;

        Ok(serde_json::from_value(res)?)
    }

    /// Request the scene mode for a specific device.
    pub fn get_device_scene_mode<S>(&self, device: S, scene_id: usize) -> Result<SceneMode>
    where
        S: Into<String>,
    {
        let json = self.generic_request(
            "device/getSceneMode",
            Some(vec![
                ("dsid", &device.into()),
                ("sceneID", &scene_id.to_string()),
            ]),
        )?;

        // convert to SceneMode
        Ok(serde_json::from_value(json)?)
    }

    /// Get all available circuts
    pub fn get_circuits(&self) -> Result<Vec<Circut>> {
        let mut res = self.generic_request("apartment/getCircuits", None)?;

        let res = res
            .get_mut("circuits")
            .ok_or("No circuits available")?
            .take();

        Ok(serde_json::from_value(res)?)
    }

    /// Get all available scenes for a specific zone with a type.
    pub fn get_scenes(&self, zone: usize, typ: Type) -> Result<Vec<usize>> {
        // convert the enum to usize
        let typ = typ as usize;

        let mut json = self.generic_request(
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

    /// Return the last called scene for a zone.
    pub fn get_last_called_scene(&self, zone: usize, typ: Type) -> Result<usize> {
        // convert the enum to usize
        let typ = typ as usize;

        let res = self.generic_request(
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

    /// Trigger a scene for a specific zone and type in the dss system.
    pub fn call_scene(&self, zone: usize, typ: Type, scene: usize) -> Result<()> {
        // convert the enum to usize
        let typ = typ as usize;

        self.generic_request(
            "zone/callScene",
            Some(vec![
                ("id", &zone.to_string()),
                ("groupID", &typ.to_string()),
                ("sceneNumber", &scene.to_string()),
            ]),
        )?;

        Ok(())
    }

    /// Transforms a action to a scene call if possible and executes it
    pub fn call_action(&self, zone: usize, action: Action) -> Result<()> {
        // transform the action to a typ and scene
        let (typ, scene) = action
            .to_scene_type()
            .ok_or("Action can't be transformed to scene command")?;
        self.call_scene(zone, typ, scene)
    }

    /// Get the opening status of a single shadow device and resturns it.
    pub fn get_shadow_device_open<S>(&self, device: S) -> Result<f32>
    where
        S: Into<String>,
    {
        // make the request
        let res = self.generic_request(
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

    /// Set the shadow opening for a single device
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
        self.generic_request(
            "device/setOutputValue",
            Some(vec![
                ("dsid", &device.into()),
                ("value", &format!("{}", value)),
                ("offset", "2"),
            ]),
        )?;

        Ok(())
    }

    /// Get the shadow open angle for a single device.
    pub fn get_shadow_device_angle<S>(&self, device: S) -> Result<f32>
    where
        S: Into<String>,
    {
        // make the request
        let res = self.generic_request(
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

    /// Set the shade open angle for a single device
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
        self.generic_request(
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

/// Takes a String input and tries to convert it to the needed
/// format requested by the struct.
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

/// The event get fired by the digital strom server, whenever
/// a scene was called.
///
/// A scene get's called when a switch is pressed in the appartment or
/// a similar action get triggered. The direct set of
/// shadow opennings or angles are getting not received.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Event {
    #[serde(default)]
    pub name: String,

    #[serde(rename = "zoneID", deserialize_with = "from_str")]
    pub zone: usize,

    #[serde(rename = "groupID", deserialize_with = "from_str")]
    pub typ: Type,

    #[serde(rename = "sceneID", deserialize_with = "from_str")]
    pub scene: usize,

    #[serde(rename = "originToken")]
    pub token: String,

    #[serde(rename = "originDSUID")]
    pub dsuid: String,

    #[serde(rename = "callOrigin")]
    pub origin: String,

    #[serde(default)]
    pub action: Action,

    #[serde(default)]
    pub value: Value,

    #[serde(default)]
    pub group: usize,
}

/// A zone is like a room or sub-room in an appartment.
/// It has a definable name and groups.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Zone {
    #[serde(rename = "zoneID")]
    pub id: usize,
    pub name: String,
    #[serde(rename = "groups")]
    pub types: Vec<Type>,
    #[serde(default, rename = "dssGroups")]
    pub groups: Vec<Group>,
}

/// The type definition is used for a group to determine what it controlls
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
    /// Transform a unsigned integer representation towards a DSS Type.
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

/// This object can be directly created by serde, when a string was given.
impl std::str::FromStr for Type {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let u = u8::from_str(s)?;
        Ok(Type::from(u))
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Light => write!(f, "Light"),
            Type::Shadow => write!(f, "Shadow"),
            Type::Heating => write!(f, "Heating"),
            Type::Audio => write!(f, "Audio"),
            Type::Video => write!(f, "Video"),
            Type::Joker => write!(f, "Joker"),
            Type::Cooling => write!(f, "Cooling"),
            Type::Ventilation => write!(f, "Ventilation"),
            Type::Window => write!(f, "Window"),
            Type::AirRecirculation => write!(f, "AirRecirculation"),
            Type::TemperatureControl => write!(f, "TemperatureControl"),
            Type::ApartmentVentilation => write!(f, "ApartmentVentilation"),
            Type::Unknown => write!(f, "Unknown"),
        }
    }
}

/// An action defines what has happend to a specific group or what should happen.
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

    fn to_scene_type(&self) -> Option<(Type, usize)> {
        match self {
            Action::AllLightOff => Some((Type::Light, 0)),
            Action::AllLightOn => Some((Type::Light, 5)),
            Action::LightOff(v) => Some((Type::Light, *v)),
            Action::LightOn(v) => Some((Type::Light, v + 5)),

            Action::AllShadowDown => Some((Type::Shadow, 0)),
            Action::AllShadowUp => Some((Type::Shadow, 5)),
            Action::AllShadowStop => Some((Type::Shadow, 55)),
            Action::AllShadowSpecial1 => Some((Type::Shadow, 18)),
            Action::AllShadowSpecial2 => Some((Type::Shadow, 19)),

            Action::ShadowDown(v) => Some((Type::Shadow, *v)),
            Action::ShadowUp(v) => Some((Type::Shadow, v + 5)),
            Action::ShadowStop(v) => Some((Type::Shadow, v + 51)),
            Action::ShadowStepClose => Some((Type::Shadow, 42)),
            Action::ShadowStepOpen => Some((Type::Shadow, 43)),

            Action::Unknown => None,
        }
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

/// The Value objects describes which status a group has. It is also used to
/// set the new status of a group.
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

    pub fn as_bool(&self) -> bool {
        match self {
            Value::Light(v) => {
                if v < &0.5 {
                    false
                } else {
                    true
                }
            }
            _ => false,
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Unknown
    }
}

/// A specific device which is used within a group
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    #[serde(rename = "zoneID")]
    pub zone_id: usize,
    #[serde(rename = "isPresent")]
    pub present: bool,
    #[serde(rename = "outputMode")]
    pub device_type: DeviceType,
    #[serde(rename = "groups")]
    pub types: Vec<Type>,
    #[serde(rename = "buttonActiveGroup")]
    pub button_type: Type,
}

/// The device type describes, what kind of device is avilable.
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

/// A circut is a device which provides meter functionality within a dss installation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Circut {
    #[serde(rename = "dsid")]
    pub id: String,
    pub name: String,
    #[serde(rename = "isPresent")]
    pub present: bool,
    #[serde(rename = "isValid")]
    pub valid: bool,
}

/// Represents all special scene stats.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SceneMode {
    #[serde(rename = "sceneID")]
    pub scene: usize,
    #[serde(rename = "dontCare")]
    pub dont_care: bool,
    #[serde(rename = "localPrio")]
    pub local_prio: bool,
    #[serde(rename = "specialMode")]
    pub special_mode: bool,
    #[serde(rename = "flashMode")]
    pub flash_mode: bool,
    #[serde(rename = "ledconIndex")]
    pub led_con_index: usize,
}

/// A Group which is located in a zone and holds all the single devices
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Group {
    pub id: usize,
    pub zone_id: usize,
    pub typ: Type,
    pub status: Value,
    pub devices: Vec<Device>,
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
        let groups: Vec<Group> = scenes
            .iter()
            .filter_map(|s| Group::from_scene(*s, zone_id, typ))
            .collect();

        if groups.len() > 1 {
            return groups.into_iter().filter(|g| g.id > 0).collect();
        }

        return groups;
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

/// DSS error type to collect all the avilable errors which can occour.
#[derive(Debug)]
pub enum Error {
    Error(String),
    SerdeJson(serde_json::Error),
    Reqwest(reqwest::Error),
    Io(std::io::Error),
}

/// Short return type for the DSS Error
type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Error(s) => write!(f, "{}", s),
            Error::SerdeJson(ref e) => e.fmt(f),
            Error::Reqwest(ref e) => e.fmt(f),
            Error::Io(ref e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        match self {
            Error::Error(s) => &s,
            Error::SerdeJson(ref e) => e.description(),
            Error::Reqwest(ref e) => e.description(),
            Error::Io(ref e) => e.description(),
        }
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        match self {
            Error::Error(_) => None,
            Error::SerdeJson(ref e) => Some(e),
            Error::Reqwest(ref e) => Some(e),
            Error::Io(ref e) => Some(e),
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

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
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
