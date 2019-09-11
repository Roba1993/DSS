fn main() {
    let api = Api::new("dss", "dssadmin", "dssadmin").unwrap();
    println!("{:?}", api);


    println!(
        "{:?}",
        api.plain_request("apartment/setName", Some(vec!(("newName", "dss"))))
    );
}

#[derive(Debug)]
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

        // return the json response
        Ok(response.json()?)
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