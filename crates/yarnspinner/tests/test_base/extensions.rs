//! Extension traits that make testing easier

use crate::prelude::*;
use yarnspinner_compiler::prelude::*;

pub trait TestCompiler {
    /// Compiles `source` as the body of a node called `Start`.
    fn from_test_source(source: &str) -> Self;
    /// Compiles `source` as a complete file.
    fn from_source(source: &str) -> Self;
}

impl TestCompiler for Compiler {
    fn from_test_source(source: &str) -> Self {
        let file = File {
            file_name: "<input>".to_string(),
            source: create_test_node(source),
        };

        let mut compiler = Self::new();
        compiler.add_file(file);
        compiler
    }

    fn from_source(source: &str) -> Self {
        let mut compiler = Self::new();
        compiler.add_file(File {
            file_name: "<input>".to_string(),
            source: source.to_owned(),
        });
        compiler
    }
}
