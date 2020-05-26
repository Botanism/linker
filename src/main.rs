#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;

use rocket::http::Status;
use rocket_contrib::json::Json;
use serde_json::Value;
use std::fs;
use std::path::Path;

const BASE_PATH: &str = "emulated/";
const SERVER_PATH: &str = "emulated/servers/";

/// Returns the IDs of guilds handled by the bot.
fn get_guilds() -> Vec<String> {
    Path::new(SERVER_PATH)
        .read_dir()
        .expect("SERVER_PATH does not exist!")
        .map(|file| {
            file.expect("Could not read files in SERVER_PATH")
                .file_name()
                .into_string()
                .unwrap() //the value written by python here is always UTF-8 valid
                .split(".")
                .next()
                .unwrap() //there will always at least be one value in this collection, even if for some reason the filename did not contain a `.`
                .to_string()
        })
        .collect()
}

/// Returns an array sttrings of guild IDs. Each ID represents a guild handled by the bot.
#[get("/")]
pub fn server_all() -> Json<Value> {
    Json(Value::Array(
        get_guilds()
            .iter()
            .map(|id| Value::String(id.to_string()))
            .collect(),
    ))
}

/// Returns a JSON bool denoting the existence of a config for this guild
#[get("/server/<gid>")]
pub fn server_one(gid: String) -> Json<bool> {
    let mut file = String::from(gid);
    file.push_str(".json");
    Json(Path::new(SERVER_PATH).join(file).exists())
}

/// Returns the JSON config file for the specified guild
#[get("/config/<gid>")]
pub fn server_conf(gid: String) -> Option<Json<Value>> {
    let mut path = String::from(SERVER_PATH);
    path.push_str(&gid);
    path.push_str(".json");
    let contents = fs::read_to_string(path).ok()?;
    Some(Json(serde_json::from_str(&contents).ok()?))
}

/// returns a JSON array of available languages for the bot
#[get("/langs")]
pub fn available_langs() -> Json<Value> {
    let path = Path::new(BASE_PATH).join("settings.py");
    let contents = fs::read_to_string(path).expect("settings.py is missing!");

    println!("{:?}", contents);
    let langs_line: Vec<_> = contents
        .lines()
        .filter(|line| line.starts_with("ALLOWED_LANGS"))
        .collect();
    println!("{:?}", langs_line);
    assert_eq!(langs_line.len(), 1); //langs shouldn't be defined more than once
    Json(
        serde_json::from_str(langs_line[0].rsplit("=").next().unwrap())
            .expect("Could not decode python list to json array!"),
    )
}

/// Reloads the langs, currently not needed. Only here for backward-compatibility
#[get("/reload")]
fn reload_langs() -> Json<bool> {
    Json(true)
}

/// attempts to update the config file for guild with id <gid>
/// retuns different status codes depending on the results:
///     200 if successful
///     404 if <gid> is invalid
///     304 if updating failed for some internal reason
#[put("/update/<gid>", format = "json", data = "<conf>")]
pub fn overwrite_conf(gid: String, conf: Json<serde_json::Value>) -> Status {
    let path = Path::new(SERVER_PATH);
    if !get_guilds().contains(&gid) {
        return Status::NotFound;
    } else {
        match fs::write(path, conf.to_string()) {
            Ok(_) => Status::Ok,
            Err(_) => Status::NotModified,
        }
    }
}

pub fn get_rocket() -> rocket::Rocket {
    rocket::ignite().mount(
        "/",
        routes![
            server_one,
            server_all,
            server_conf,
            available_langs,
            overwrite_conf,
            reload_langs
        ],
    )
}

fn main() {
    get_rocket().launch();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::http::{ContentType, Status};
    use rocket::local::Client;
    use std::sync::Once;

    static INIT: Once = Once::new();
    fn setup_env() {
        INIT.call_once(|| {
            fs::create_dir_all(SERVER_PATH).expect("Couldn't setup environement");
            fs::write(
                Path::new(BASE_PATH).join("settings.py"),
                "ALLOWED_LANGS = [\"en\",\"fr\"]",
            )
            .expect("Couldn't setup environement");
            fs::write(Path::new(SERVER_PATH).join("54654564.json"), "{}")
                .expect("Couldn't create server file");
        })
    }

    #[test]
    fn test_server_all() {
        setup_env();
        let client = Client::new(get_rocket()).expect("Got an invalid rocket instance");
        let mut response = client.get("/").dispatch();
        assert_eq!(response.content_type(), Some(ContentType::JSON));
        assert_eq!(response.body_string(), Some(String::from("[\"54654564\"]")));
    }

    #[test]
    fn test_server_one() {
        setup_env();
        let client = Client::new(get_rocket()).expect("Got an invalid rocket instance");
        let mut response = client.get("/server/54654564").dispatch();
        assert_eq!(response.body_string(), Some(String::from("true")));
        let mut response = client.get("/server/wrong_id").dispatch();
        assert_eq!(response.body_string(), Some(String::from("false")));
    }

    #[test]
    fn test_server_conf() {
        setup_env();
        let client = Client::new(get_rocket()).expect("Got an invalid rocket instance");
        let mut response = client.get("/config/54654564").dispatch();
        assert_eq!(response.body_string(), Some(String::from("{}")));
    }

    #[test]
    fn test_available_langs() {
        setup_env();
        let client = Client::new(get_rocket()).expect("Got an invalid rocket instance");
        let mut response = client.get("/langs").dispatch();
        assert_eq!(
            response.body_string(),
            Some(String::from("[\"en\",\"fr\"]"))
        );
    }

    #[test]
    fn test_overwrite_conf() {
        setup_env();
        let client = Client::new(get_rocket()).expect("Got an invalid rocket instance");
        let mut response = client.put("/update/54654564").dispatch();
        assert_eq!(response.status(), Status::Ok);
    }
}
