//! Script only the external server responses, OS observations and credential
//! store. Tests still execute the production service, validation and decoder.
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde_json::{Value, json};

use super::{Api, ApiError, Reply, parse_reply};

pub(crate) struct Step {
    pub(crate) function: &'static str,
    pub(crate) status: u16,
    pub(crate) body: Value,
}

pub(crate) fn step(function: &'static str, data: Value) -> Step {
    Step {
        function,
        status: 200,
        body: json!({"data": data}),
    }
}

pub(crate) struct Fixture {
    steps: Mutex<VecDeque<Step>>,
    pub(crate) calls: Mutex<Vec<(&'static str, Value)>>,
    pub(crate) checks: Mutex<Vec<bool>>,
    pub(crate) names: Mutex<VecDeque<Result<String, String>>>,
    saved_name: Mutex<Option<String>>,
    listener_closed: AtomicBool,
}

impl Fixture {
    pub(crate) fn api(steps: Vec<Step>) -> Api {
        let mut api = Api::new(super::super::context::Endpoint::fixture()).unwrap();
        api.test_credentials = Some(std::sync::Arc::new(Mutex::new(BTreeMap::new())));
        api.fixture = Some(Self {
            steps: Mutex::new(steps.into()),
            calls: Mutex::new(Vec::new()),
            checks: Mutex::new(Vec::new()),
            names: Mutex::new(VecDeque::new()),
            saved_name: Mutex::new(None),
            listener_closed: AtomicBool::new(false),
        });
        api
    }

    pub(crate) fn check(&self, process_only: bool) -> Result<(), String> {
        self.checks.lock().unwrap().push(process_only);
        if !process_only && self.listener_closed.load(Ordering::SeqCst) {
            return Err("The external listener closed during map loading.".into());
        }
        Ok(())
    }

    pub(crate) fn name(&self) -> Result<String, String> {
        self.names
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok("Fixture".into()))
    }

    pub(crate) fn reply(&self, function: &'static str, data: &Value) -> Result<Reply, ApiError> {
        self.calls.lock().unwrap().push((function, data.clone()));
        if function == "SaveGame" {
            *self.saved_name.lock().unwrap() = data
                .get("SaveName")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("An unexpected server request was made.");
        assert_eq!(step.function, function);
        let mut body = step.body;
        fn substitute(value: &mut Value, saved_name: &Option<String>) {
            match value {
                Value::String(text) if text == "$saved" => {
                    *text = saved_name.clone().expect("SaveGame must run first.")
                }
                Value::Array(values) => {
                    for value in values {
                        substitute(value, saved_name);
                    }
                }
                Value::Object(values) => {
                    for value in values.values_mut() {
                        substitute(value, saved_name);
                    }
                }
                _ => {}
            }
        }
        substitute(&mut body, &self.saved_name.lock().unwrap());
        if matches!(function, "CreateNewGame" | "LoadGame") && step.status == 202 {
            self.listener_closed.store(true, Ordering::SeqCst);
        }
        parse_reply(step.status, &serde_json::to_vec(&body).unwrap())
    }

    pub(crate) fn assert_finished(&self) {
        assert!(self.steps.lock().unwrap().is_empty());
    }
}
