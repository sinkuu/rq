use std::collections;
use std::mem;
use std::path;

use duk;

use error;
use query;
use value;

const API_JS: &'static str = include_str!("../api.js");
const PRELUDE_JS: &'static str = include_str!("../prelude.js");

const MODULES: &'static [(&'static str, &'static str)] =
    &[("jsonpath.js", include_str!("../js/jsonpath.min.js")),
      ("lodash.js", include_str!("../js/lodash.custom.min.js")),
      ("minieval.js", include_str!("../js/minieval.min.js"))];

#[derive(Debug)]
pub struct Context {
    duk: duk::Context,
}

#[derive(Debug)]
pub struct Process<'a> {
    duk: &'a duk::Context,
    thread: duk::Reference<'a>,
    process: duk::Reference<'a>,
    resume: duk::Reference<'a>,
    state: State,
}

#[derive(Debug)]
pub enum State {
    Start,
    Await,
    Emit(value::Value),
    End,
}

impl Context {
    pub fn new() -> Context {
        let ctx = duk::Context::builder()
            .with_module_resolver(Box::new(Context::resolve_module))
            .with_module_loader(Box::new(Context::load_module))
            .build();

        ctx.eval_string_with_filename("api.js", API_JS).unwrap();
        ctx.eval_string_with_filename("prelude.js", PRELUDE_JS).unwrap();

        Context { duk: ctx }
    }

    pub fn process(&self, name: &str) -> error::Result<Process> {
        let global = self.duk.global_object();
        let func = try!(global.get(name));

        if let duk::Value::Undefined = func.to_value() {
            return Err(error::ErrorKind::ProcessNotFound(name.to_owned()).into());
        }

        let rq_ns = try!(global.get("rq"));
        let process_ctor = try!(rq_ns.get("Process"));
        let process = try!(process_ctor.new(&[&func]));
        let run = try!(process.get("run"));
        let resume = try!(process.get("resume"));

        let duktape_ns = try!(global.get("Duktape"));
        let thread_ctor = try!(duktape_ns.get("Thread"));
        let thread = try!(thread_ctor.new(&[&run]));

        Ok(Process {
            duk: &self.duk,
            thread: thread,
            process: process,
            resume: resume,
            state: State::Start,
        })
    }

    fn resolve_module(name: String, context: String) -> String {
        debug!("Resolving module {:?} from {:?}", name, context);
        String::from(&*path::Path::new(&context).join(name).with_extension("js").to_string_lossy())
    }

    fn load_module(canonical_name: String) -> Option<String> {
        for &(name, data) in MODULES {
            if &canonical_name == name {
                debug!("Loading JS module {:?}", name);
                return Some(data.to_owned());
            }
        }
        warn!("Could not load JS module {:?}", canonical_name);
        None
    }
}

impl<'a> Process<'a> {
    pub fn is_start(&self) -> bool {
        match self.state {
            State::Start => true,
            _ => false,
        }
    }

    pub fn is_await(&self) -> bool {
        match self.state {
            State::Await => true,
            _ => false,
        }
    }

    pub fn is_emit(&self) -> bool {
        match self.state {
            State::Emit(_) => true,
            _ => false,
        }
    }

    pub fn is_end(&self) -> bool {
        match self.state {
            State::End => true,
            _ => false,
        }
    }

    pub fn run_start(&mut self, args: &[query::Expression]) -> error::Result<()> {
        if let State::Start = self.state {
            let global = self.duk.global_object();
            let rq_ns = try!(global.get("rq"));
            let create_function = try!(rq_ns.get("createFunction"));

            let array_ctor = try!(global.get("Array"));
            let array = try!(array_ctor.new(&[]));

            for arg in args {
                match arg {
                    &query::Expression::Value(ref v) => {
                        let v = value_to_duk(v.clone());
                        try!(array.call_method("push", &[&v]));
                    },
                    &query::Expression::Function(ref args, ref body) => {
                        let args = duk::Value::Array(args.iter()
                            .map(|arg| duk::Value::String(arg.clone()))
                            .collect());
                        let body = duk::Value::String(body.clone());
                        let function = try!(create_function.call(&[&args, &body]));
                        try!(array.call_method("push", &[&function]));
                    },
                }
            }

            let result = try!(self.resume.call(&[&self.thread, &array]));
            try!(self.handle_yield(&result));
            Ok(())
        } else {
            panic!("Not in Start state");
        }
    }

    pub fn run_await(&mut self, next: Option<value::Value>) -> error::Result<()> {
        if let State::Await = self.state {
            let mut o = collections::BTreeMap::new();
            if let Some(v) = next {
                o.insert("hasNext".to_owned(), duk::Value::Boolean(true));
                o.insert("next".to_owned(), value_to_duk(v));
            } else {
                o.insert("hasNext".to_owned(), duk::Value::Boolean(false));
            };
            let object = duk::Value::Object(o);
            let result = try!(self.resume.call(&[&self.thread, &object]));
            try!(self.handle_yield(&result));
            Ok(())
        } else {
            panic!("Not in Await state");
        }
    }

    pub fn run_emit(&mut self) -> error::Result<value::Value> {
        if let State::Emit(v) = mem::replace(&mut self.state, State::End) {
            let result = try!(self.resume.call(&[&self.thread]));
            try!(self.handle_yield(&result));
            Ok(v)
        } else {
            panic!("Not in Emit state");
        }
    }

    fn handle_yield(&mut self, result: &duk::Reference) -> error::Result<()> {
        let mut value = result.to_value();
        match value {
            duk::Value::Object(ref mut o) => {
                match o.remove("type") {
                    Some(duk::Value::String(t)) => {
                        match t.as_str() {
                            "await" => {
                                trace!("Process entering await state");
                                self.state = State::Await;
                                Ok(())
                            },
                            "emit" => {
                                if let Some(v) = o.remove("value") {
                                    trace!("Process entering emit state");
                                    self.state = State::Emit(value_from_duk(v));
                                    Ok(())
                                } else {
                                    let msg = format!("No value specified for emitting");
                                    Err(error::Error::illegal_state(msg))
                                }
                            },
                            t => {
                                let msg = format!("Unexpected coroutine message type: {:?}", t);
                                Err(error::Error::illegal_state(msg))
                            },
                        }
                    },
                    t => {
                        let msg = format!("Expected a coroutine message to have a string type, \
                                           but it was {:?}",
                                          t);
                        Err(error::Error::illegal_state(msg))
                    },
                }
            },
            duk::Value::Undefined => {
                trace!("Process entering end state");
                self.state = State::End;
                Ok(())
            },
            _ => {
                let msg = format!("Unexpected return value from Javascript function: {:?}",
                                  value);
                Err(error::Error::illegal_state(msg))
            },
        }
    }
}

fn value_from_duk(value: duk::Value) -> value::Value {
    match value {
        duk::Value::Null | duk::Value::Undefined => value::Value::Unit,
        duk::Value::Boolean(v) => value::Value::Bool(v),
        duk::Value::Number(v) => {
            if (v as u8) as f64 == v {
                value::Value::U8(v as u8)
            } else if (v as u16) as f64 == v {
                value::Value::U16(v as u16)
            } else if (v as u32) as f64 == v {
                value::Value::U32(v as u32)
            } else if (v as u64) as f64 == v {
                value::Value::U64(v as u64)
            } else if (v as usize) as f64 == v {
                value::Value::USize(v as usize)
            } else if (v as i8) as f64 == v {
                value::Value::I8(v as i8)
            } else if (v as i16) as f64 == v {
                value::Value::I16(v as i16)
            } else if (v as i32) as f64 == v {
                value::Value::I32(v as i32)
            } else if (v as i64) as f64 == v {
                value::Value::I64(v as i64)
            } else if (v as isize) as f64 == v {
                value::Value::ISize(v as isize)
            } else if (v as f32) as f64 == v {
                value::Value::from_f32(v as f32)
            } else {
                value::Value::from_f64(v)
            }
        },
        duk::Value::String(v) => value::Value::String(v),
        duk::Value::Array(v) => value::Value::Sequence(v.into_iter().map(value_from_duk).collect()),
        duk::Value::Object(v) => {
            value::Value::Map(v.into_iter()
                .map(|(k, v)| (value::Value::String(k), value_from_duk(v)))
                .collect())
        },
        duk::Value::Bytes(v) => value::Value::Bytes(v),
        duk::Value::Foreign(_) => value::Value::Unit,
    }
}

fn value_to_duk(value: value::Value) -> duk::Value {
    match value {
        value::Value::Unit => duk::Value::Null,
        value::Value::Bool(v) => duk::Value::Boolean(v),

        value::Value::ISize(v) => duk::Value::Number(v as f64),
        value::Value::I8(v) => duk::Value::Number(v as f64),
        value::Value::I16(v) => duk::Value::Number(v as f64),
        value::Value::I32(v) => duk::Value::Number(v as f64),
        value::Value::I64(v) => duk::Value::Number(v as f64),

        value::Value::USize(v) => duk::Value::Number(v as f64),
        value::Value::U8(v) => duk::Value::Number(v as f64),
        value::Value::U16(v) => duk::Value::Number(v as f64),
        value::Value::U32(v) => duk::Value::Number(v as f64),
        value::Value::U64(v) => duk::Value::Number(v as f64),

        value::Value::F32(v) => duk::Value::Number(v.0 as f64),
        value::Value::F64(v) => duk::Value::Number(v.0 as f64),

        value::Value::Char(v) => duk::Value::String(format!("{}", v)),
        value::Value::String(v) => duk::Value::String(v),
        value::Value::Bytes(v) => duk::Value::Bytes(v),

        value::Value::Sequence(v) => duk::Value::Array(v.into_iter().map(value_to_duk).collect()),
        value::Value::Map(v) => {
            duk::Value::Object(v.into_iter()
                .map(|(k, v)| (format!("{}", k), value_to_duk(v)))
                .collect())
        },
    }
}
