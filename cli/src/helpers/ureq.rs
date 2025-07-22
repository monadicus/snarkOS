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

use serde::de::DeserializeOwned;
use ureq::{Body, Error, http::Response};

/// Used to return the response body on Error (which was removed by default).
/// See [here](https://github.com/algesten/ureq/issues/997#issuecomment-2658534447) for more details.
pub(crate) fn handle_ureq_result(result: Result<Response<Body>, Error>) -> Result<Body, (Error, Option<String>)> {
    match result {
        Ok(response) => {
            let status = response.status();
            let mut body = response.into_body();

            if status.is_success() {
                Ok(body)
            } else {
                let message = match body.read_to_string() {
                    Ok(msg) => msg,
                    Err(err) => err.to_string(),
                };

                Err((ureq::Error::StatusCode(status.as_u16()), Some(message)))
            }
        }
        Err(err) => Err((err, None)),
    }
}

/// Issue a HTTP request and parse the response.
pub(crate) fn http_get(path: &str) -> Result<Body, (Error, Option<String>)> {
    handle_ureq_result(ureq::get(path).config().http_status_as_error(false).build().call())
}

/// Issue a HTTP request, parse the response, and return it as JSON.
pub(crate) fn http_get_json<T: DeserializeOwned>(path: &str) -> Result<T, (Error, Option<String>)> {
    http_get(path)?.read_json().map_err(|err| (err, None))
}
