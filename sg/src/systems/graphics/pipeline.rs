use std::{collections::HashMap, lazy::SyncLazy, path::Path};

use codespan_reporting::{
    diagnostic::{Diagnostic, Label},
    files::SimpleFiles,
    term::termcolor::StandardStream,
};
use regex::Regex;

pub enum ShaderConstant {
    Integer(i64),
    Float(f64),
    Bool(bool),
    Any(String),
}

impl ToString for ShaderConstant {
    fn to_string(&self) -> String {
        match self {
            Self::Integer(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Bool(b) => b.to_string(),
            Self::Any(a) => a.to_string(),
        }
    }
}

pub struct Shader {
    name: &'static str,
    source: String,
    constants: HashMap<&'static str, ShaderConstant>,
}

#[macro_export]
macro_rules! include_shader {
    ($path:literal, $name:literal) => {
        $crate::systems::graphics::pipeline::Shader::new(include_str!($path).to_owned(), $name)
    };
}

impl<'a> Shader {
    pub fn from_file(path: impl AsRef<Path>, name: &'static str) -> Self {
        Self::new(
            std::fs::read_to_string(path).expect("Error on file read"),
            name,
        )
    }
    pub fn new(source: String, name: &'static str) -> Self {
        Self {
            name,
            source,
            constants: HashMap::new(),
        }
    }
    pub fn set(&mut self, key: &'static str, value: ShaderConstant) {
        self.constants.insert(key, value);
    }
    pub fn set_integer(&mut self, key: &'static str, value: i64) {
        self.set(key, ShaderConstant::Integer(value));
    }
    pub fn set_float(&mut self, key: &'static str, value: f64) {
        self.set(key, ShaderConstant::Float(value));
    }
    pub fn set_bool(&mut self, key: &'static str, value: bool) {
        self.set(key, ShaderConstant::Bool(value));
    }
    pub fn get(&self, key: &'static str) -> Option<&ShaderConstant> {
        self.constants.get(key)
    }
    pub fn get_integer(&self, key: &'static str) -> Option<i64> {
        match self.constants.get(key)? {
            ShaderConstant::Integer(i) => Some(*i),
            _ => None,
        }
    }
    pub fn get_float(&self, key: &'static str) -> Option<f64> {
        match self.constants.get(key)? {
            ShaderConstant::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn get_bool(&self, key: &'static str) -> Option<bool> {
        match self.constants.get(key)? {
            ShaderConstant::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn module(&self, device: &wgpu::Device) -> wgpu::ShaderModule {
        let mut source = self.source.to_owned();
        let mut pat = "{{_}}".to_owned();
        for (p, val) in &self.constants {
            pat.replace_range(2..(pat.len() - 2), p);
            source = source.replace(&pat, &val.to_string());
        }
        // check for unset constants in debug builds
        #[cfg(debug_assertions)]
        {
            let mut err_count = 0;
            let mut files = SimpleFiles::new();
            let file = files.add(self.name, &source);
            let writer =
                StandardStream::stderr(codespan_reporting::term::termcolor::ColorChoice::Always);
            let config = codespan_reporting::term::Config::default();
            static RE: SyncLazy<Regex> = SyncLazy::new(|| Regex::new(r"\{\{(.+?)\}\}").unwrap());
            for cap in RE.captures_iter(&source) {
                err_count += 1;
                let m = cap.get(1).unwrap();
                let diagnostic = Diagnostic::error()
                    .with_message("constant hasn't been given any value")
                    .with_labels(vec![Label::primary(file, m.range())
                        .with_message(format!("No value for `{}` given", m.as_str()))]);
                codespan_reporting::term::emit(&mut writer.lock(), &config, &files, &diagnostic)
                    .ok();
            }
            if err_count > 0 {
                panic!(
                    "Error{} in shader preprocessing.",
                    if err_count == 1 { "" } else { "s" }
                )
            }
        }
        device.create_shader_module(&wgpu::ShaderModuleDescriptor {
            source: wgpu::ShaderSource::Wgsl(source.into()),
            label: Some(self.name),
        })
    }
}

pub struct Pipeline {
    layout: wgpu::PipelineLayout,
    build: Box<
        dyn Fn(&wgpu::Device, &wgpu::PipelineLayout, &wgpu::ShaderModule) -> wgpu::RenderPipeline,
    >,
    pub pipeline: wgpu::RenderPipeline,
    pub shader: Shader,
}

impl Pipeline {
    pub fn new<F>(
        device: &wgpu::Device,
        layout: wgpu::PipelineLayout,
        shader: Shader,
        build: F,
    ) -> Self
    where
        F: Fn(&wgpu::Device, &wgpu::PipelineLayout, &wgpu::ShaderModule) -> wgpu::RenderPipeline
            + 'static,
    {
        let pipeline = build(device, &layout, &shader.module(device));
        let build = Box::new(build);
        Self {
            layout,
            build,
            pipeline,
            shader,
        }
    }

    pub fn rebuild(&mut self, device: &wgpu::Device) {
        self.pipeline = (self.build)(device, &self.layout, &self.shader.module(device));
    }
}
