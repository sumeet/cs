use itertools::Itertools;
use matches::matches;
use serde_json;
use std::collections::BTreeMap;

use super::env_genie::EnvGenie;
use super::json2;
use super::structs;
use super::lang;
use super::http_request;
use super::json_http_client;
use super::async_executor::AsyncExecutor;
use super::result::{Result as EZResult};

#[derive(Clone)]
pub struct JSONHTTPClientBuilder {
    pub test_url: String,
    pub test_run_result: Option<Result<serde_json::Value,String>>,
    pub test_run_parsed_doc: Option<json2::ParsedDocument>,
    pub json_http_client_id: lang::ID,
    pub selected_fields: Vec<SelectedField>,
}

#[derive(Clone)]
pub struct SelectedField {
    pub name: String,
    pub nesting: json2::Nesting,
    pub typespec_id: lang::ID,
}

impl JSONHTTPClientBuilder {
    pub fn new(json_http_client_id: lang::ID) -> Self {
        Self {
            test_url: "https://httpbin.org/get".to_string(),
            test_run_result: None,
            test_run_parsed_doc: None,
            json_http_client_id,
            selected_fields: vec![]
        }
    }

    pub fn get_selected_field(&self, nesting: &json2::Nesting) -> Option<&SelectedField> {
        self.selected_fields.iter()
            .find(|field| &field.nesting == nesting)
    }

    pub fn add_selected_field(&mut self, nesting: json2::Nesting) {
        let field = self.test_run_parsed_doc.as_ref().unwrap().find(&nesting)
            .expect("couldn't find field for some reason");
        self.selected_fields.push(SelectedField {
            name: gen_field_name(&nesting),
            nesting,
            typespec_id: get_typespec_id(field),
        })
    }

    pub fn remove_selected_field(&mut self, nesting: json2::Nesting) {
        self.selected_fields
            .drain_filter(|field| field.nesting == nesting);
    }

    fn set_test_result(&mut self, result: Result<serde_json::Value,String>) {
        self.test_run_result = Some(result.clone());
        if let Ok(value) = result {
            self.test_run_parsed_doc = Some(json2::parse(value))
        } else {
            self.test_run_parsed_doc = None
        }
        self.selected_fields.clear()
    }

    pub fn run_test<F: FnOnce(JSONHTTPClientBuilder) + 'static>(&self, async_executor: &mut AsyncExecutor,
                                                                callback: F) {
        let url = self.test_url.clone();
        let mut new_builder = self.clone();
        async_executor.exec(async move {
            let val = await!(do_get_request(url));
            let result = val.map_err(|e| e.to_string());
            new_builder.set_test_result(result);
            callback(new_builder);
            let ok : Result<(), ()> = Ok(());
            ok
        });
    }
}

fn gen_field_name(nesting: &json2::Nesting) -> String {
    nesting.iter()
        .filter_map(|n| {
            match n {
                json2::Nest::MapKey(key) => Some(key.as_str()),
                _ => None,
            }
        }).last().unwrap_or("h00000what").to_string()
}

async fn do_get_request(url: String) -> EZResult<serde_json::Value> {
    await!(json_http_client::get_json(http_request::get(&url)?))
}

pub fn get_typespec_id(parsed_doc: &json2::ParsedDocument) -> lang::ID {
    use json2::ParsedDocument;
    match parsed_doc {
        ParsedDocument::Null { .. } => lang::NULL_TYPESPEC.id,
        ParsedDocument::Bool { .. } => lang::BOOLEAN_TYPESPEC.id,
        ParsedDocument::String { .. } => lang::STRING_TYPESPEC.id,
        ParsedDocument::Number { .. } => lang::NUMBER_TYPESPEC.id,
        ParsedDocument::NonHomogeneousCantParse { .. } |
        ParsedDocument::EmptyCantInfer { .. } |
        ParsedDocument::Map { .. } |
        ParsedDocument::List { .. } => panic!("we don't support selecting these types, smth's wrong"),
    }
}

fn make_return_type_spec(selected_fields: &Vec<SelectedField>) -> Result<ReturnTypeSpec,&str> {
    if selected_fields.is_empty() {
        return Err("no selected fields")
    }

    if selected_fields.len() == 1 && selected_fields[0].nesting.is_empty() {
        return Ok(ReturnTypeSpec::Scalar { typespec_id: selected_fields[0].typespec_id })
    }

    // TODO: i think this would need to handle arbitrarily nested lists, but for now let's
    // make it work with one
    if selected_fields.len() == 1 && selected_fields[0].nesting.len() == 1 {
        if let json2::Nest::ListElement(_) = selected_fields[0].nesting[0] {
            return Ok(ReturnTypeSpec::List(Box::new(ReturnTypeSpec::Scalar {
                    typespec_id: selected_fields[0].typespec_id
            })))
        }
    }

    if selected_fields.len() == 1 {
        return Err("don't know how to handle one field but not either a scalar or list of scalars")
    }


    unimplemented!()
}

#[derive(PartialEq, Eq, Hash)]
enum ReturnTypeSpec {
    Struct(BTreeMap<String, ReturnTypeSpec>),
    List(Box<ReturnTypeSpec>),
    Scalar { typespec_id: lang::ID },
}

struct ReturnTypeBuilder<'a> {
    built_structs: Vec<structs::Struct>,
    env_genie: &'a EnvGenie<'a>,
    return_type_spec: &'a ReturnTypeSpec,
}

impl<'a> ReturnTypeBuilder<'a> {
    pub fn new(env_genie: &'a EnvGenie<'a>, return_type: &'a ReturnTypeSpec) -> Self {
        Self { env_genie, built_structs: vec![], return_type_spec: return_type }
    }

    fn build(&mut self) -> ReturnTypeBuilderResult {
        match self.return_type_spec {
            ReturnTypeSpec::Scalar { typespec_id } => {
                ReturnTypeBuilderResult {
                    structs_to_be_added: vec![],
                    typ: lang::Type::from_spec_id(*typespec_id, vec![])
                }
            },
            ReturnTypeSpec::List(returntypespec) => {
                let mut result = ReturnTypeBuilder::new(self.env_genie, returntypespec.as_ref()).build();
                result.typ = lang::Type::with_params(&*lang::LIST_TYPESPEC, vec![result.typ]);
                result
            }
            ReturnTypeSpec::Struct(map) => {
                let struct_fields = map.iter().map(|(key, returntypespec)| {
                    let result = ReturnTypeBuilder::new(self.env_genie, returntypespec).build();
                    structs::StructField::new(key.clone(), result.typ)
                }).collect_vec();
                let mut structs_to_be_added = vec![];
                let typespec_id = self.find_existing_struct_matching(&struct_fields)
                    .map(|strukt| strukt.id)
                    .unwrap_or_else(|| {
                        let mut strukt = structs::Struct::new();
                        strukt.fields = struct_fields;
                        let id = strukt.id;
                        structs_to_be_added.push(strukt);
                        id
                    });
                ReturnTypeBuilderResult {
                    structs_to_be_added,
                    typ: lang::Type::from_spec_id(typespec_id, vec![])
                }
            }
        }
    }

    fn find_existing_struct_matching(&self, structfields: &Vec<structs::StructField>) -> Option<&structs::Struct> {
        self.env_genie.list_structs()
            .find(|strukt| {
                strukt.fields.len() == structfields.len() && {
                    normalize_struct_fields(&strukt.fields) == normalize_struct_fields(&structfields)
                }
            })
    }
}

fn normalize_struct_fields(fields: &[structs::StructField]) -> BTreeMap<&str, &lang::Type> {
    fields.iter().map(|field| (field.name.as_str(), &field.field_type)).collect()
}

struct ReturnTypeBuilderResult {
    structs_to_be_added: Vec<structs::Struct>,
    typ: lang::Type,
}
