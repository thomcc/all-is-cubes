// Copyright 2020-2022 Kevin Reid under the terms of the MIT License as detailed
// in the accompanying file README.md or <https://opensource.org/licenses/MIT>.

use std::path::PathBuf;
use std::{fmt, io};

use crate::TestId;

// TODO: better name
#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TestCombo {
    pub test_id: TestId,
    pub renderer: RendererId,
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RendererId {
    /// A single expected output expected to be equal for all renderers.
    All,

    Raytracer,
    Luminance,
    Wgpu,
}

impl fmt::Display for RendererId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RendererId::All => write!(f, "all"),
            RendererId::Raytracer => write!(f, "ray"),
            RendererId::Luminance => write!(f, "lum"),
            RendererId::Wgpu => write!(f, "wgpu"),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Version {
    Expected,
    Actual,
}

/// Returns the path at which to find a particular test data image.
///
/// This does not check whether the file exists, but it does have the side effect of
/// ensuring the directory exists.
pub(crate) fn image_path(test: &TestCombo, version: Version) -> PathBuf {
    let mut path = test_data_dir_path(version);

    // Convenience kludge: ensure the directory exists
    match std::fs::create_dir(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => panic!("Failed to create output dir '{p}': {e}", p = path.display()),
    }

    let TestCombo { test_id, renderer } = test;
    path.push(&format!("{test_id}-{renderer}.png"));

    path
}

/// Return the path to the directory in which the files of the sort specified by `version`
/// should be read or written.
pub(crate) fn test_data_dir_path(version: Version) -> PathBuf {
    // CARGO_MANIFEST_DIR will be the directory of the test-renderers package
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push(match version {
        Version::Actual => "../target/test-renderers-output/",
        Version::Expected => "expected/",
    });
    path
}