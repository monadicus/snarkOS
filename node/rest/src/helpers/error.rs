// Copyright (c) 2019-2025 Provable Inc.
// This file is part of the snarkOS library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:

// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// An error handler for the REST API server.
pub struct RestError {
    pub status: StatusCode,
    pub message: String,
}

impl RestError {
    /// Creates a new `RestError` with an internal server error status.
    pub fn new(message: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: message.into() }
    }

    /// Creates a new `RestError` with the given status code.
    pub fn with_status(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

impl IntoResponse for RestError {
    fn into_response(self) -> Response {
        (self.status, format!("Something went wrong: {}", self.message)).into_response()
    }
}

impl From<anyhow::Error> for RestError {
    fn from(err: anyhow::Error) -> Self {
        Self::new(err.to_string())
    }
}
