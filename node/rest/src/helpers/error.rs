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

/// A generic error for the REST API server.
pub struct RestError {
    /// The HTTP status code to return.
    pub status: StatusCode,
    /// The error message.
    pub message: String,
}

impl RestError {
    /// Creates a new internal server error.
    pub fn new(message: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: message.into() }
    }

    /// Creates a new error with a specific status code.
    pub fn with_status(message: impl Into<String>, status: StatusCode) -> Self {
        Self { status, message: message.into() }
    }

    /// Creates a 400 Bad Request error.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::with_status(message, StatusCode::BAD_REQUEST)
    }

    /// Creates a 404 Not Found error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::with_status(message, StatusCode::NOT_FOUND)
    }

    /// Creates a 503 Service Unavailable error.
    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::with_status(message, StatusCode::SERVICE_UNAVAILABLE)
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
