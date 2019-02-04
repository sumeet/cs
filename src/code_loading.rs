use std::fs::File;

use super::pystuff;
use super::jsstuff;
use super::structs;
use super::enums;
use super::code_function;
use super::scripts;
use super::tests;
use super::json_http_client::JSONHTTPClient;

use failure::{Error};
use serde_json;
use serde_derive::{Serialize,Deserialize};


// TODO: find a better name. til then, we're gonna save the world
#[derive(Serialize, Deserialize)]
pub struct TheWorld {
    pub scripts: Vec<scripts::Script>,
    pub codefuncs: Vec<code_function::CodeFunction>,
    pub pyfuncs: Vec<pystuff::PyFunc>,
    pub jsfuncs: Vec<jsstuff::JSFunc>,
    pub structs: Vec<structs::Struct>,
    pub enums: Vec<enums::Enum>,
    pub tests: Vec<tests::Test>,
    pub json_http_clients: Vec<JSONHTTPClient>,
}

// pub fn load(filename: &str) -> Result<CodeNode,Error> {
//     let f = File::open(filename)?;
//     Ok(serde_json::from_reader(f)?)
// }

pub fn save(filename: &str, world: &TheWorld) -> Result<(),Error> {
    let f = File::create(filename)?;
    Ok(serde_json::to_writer_pretty(f, &world)?)
}

// pub fn serialize(world: &TheWorld) -> Result<String,Error> {
//    Ok(serde_json::to_string_pretty(&world)?)
// }

pub fn deserialize(str: &str) -> Result<TheWorld,Error> {
    Ok(serde_json::from_str(str)?)
}
