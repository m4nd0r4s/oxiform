//! Times the JSON reader alone, then reader and executor without building objects:
//! `cargo run --release -p oxiform-core --example stage_timings -- document.json`.
//! A second argument, `reader` or `executor`, loops on that stage only, for a profiler.

use std::time::Instant;

use oxiform_core::compiler::compile;
use oxiform_core::description::{Description, TypeDescription};
use oxiform_core::events::{Event, EventReader};
use oxiform_core::executor::execute;
use oxiform_core::limits::Limits;
use oxiform_core::plan::{ChoiceId, ConversionId, Plan, StructId};
use oxiform_core::sink::Sink;
use oxiform_formats::json::JsonReader;

/// Builds nothing, so only reading and validation are timed.
struct NullSink;

impl Sink for NullSink {
    type Value = ();
    type Error = ();
    type Map = ();

    fn none(&mut self) -> Result<(), ()> {
        Ok(())
    }
    fn bool(&mut self, _: bool) -> Result<(), ()> {
        Ok(())
    }
    fn integer(&mut self, _: i64) -> Result<(), ()> {
        Ok(())
    }
    fn big_integer(&mut self, _: &str) -> Result<(), ()> {
        Ok(())
    }
    fn float(&mut self, _: f64) -> Result<(), ()> {
        Ok(())
    }
    fn string(&mut self, _: &str) -> Result<(), ()> {
        Ok(())
    }
    fn list(&mut self, _: Vec<()>) -> Result<(), ()> {
        Ok(())
    }
    fn tuple(&mut self, _: Vec<()>) -> Result<(), ()> {
        Ok(())
    }
    fn set(&mut self, _: Vec<()>, _: bool) -> Result<(), ()> {
        Ok(())
    }
    fn bytes(&mut self, _: &[u8], _: bool) -> Result<(), ()> {
        Ok(())
    }
    fn choice(&mut self, _: ChoiceId, _: u32) -> Result<(), ()> {
        Ok(())
    }
    fn convert(&mut self, _: ConversionId, _: ()) -> Result<Option<()>, ()> {
        Ok(Some(()))
    }
    fn map_new(&mut self) -> Result<(), ()> {
        Ok(())
    }
    fn map_insert(&mut self, _: &mut (), _: &str, _: ()) -> Result<bool, ()> {
        Ok(true)
    }
    fn map_finish(&mut self, _: ()) -> Result<(), ()> {
        Ok(())
    }
    fn instance(&mut self, _: StructId, _: &mut [Option<()>]) -> Result<(), ()> {
        Ok(())
    }
}

fn read_only(input: &str) {
    let mut reader = JsonReader::new(input, 128);
    while reader.next_event().expect("the document is valid JSON") != Event::End {}
}

fn read_and_execute(plan: &Plan, input: &str) {
    let mut reader = JsonReader::new(input, 128);
    execute(plan, &mut reader, &mut NullSink, &Limits::default()).expect("the document is valid");
}

fn time(label: &str, iterations: u32, mut run: impl FnMut()) {
    let start = Instant::now();
    for _ in 0..iterations {
        run();
    }
    let micros = start.elapsed().as_secs_f64() * 1e6 / f64::from(iterations);
    println!("{label}: {micros:.2} µs");
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments
        .next()
        .expect("usage: stage_timings document.json [reader|executor]");
    let input = std::fs::read_to_string(path).expect("the document is readable");
    let plan = compile(&Description {
        root: TypeDescription::Any,
        structs: Vec::new(),
        choices: Vec::new(),
        conversions: Vec::new(),
    })
    .expect("Any compiles");
    let iterations = 200_000;
    match arguments.next().as_deref() {
        Some("reader") => (0..iterations * 5).for_each(|_| read_only(&input)),
        Some("executor") => (0..iterations * 5).for_each(|_| read_and_execute(&plan, &input)),
        _ => {
            time("reader", iterations, || read_only(&input));
            time("reader + executor (Any, no objects)", iterations, || {
                read_and_execute(&plan, &input);
            });
        }
    }
}
