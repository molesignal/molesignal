// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `Error` 与 gRPC `Status` 之间的对应关系。内部 RPC 的两端都用它，调用方看到的错误类别
//! （以及 HTTP 状态码）与在本地执行时相同。

use tonic::{Code, Status};

use crate::shared::Error;

pub fn error_to_status(error: Error) -> Status {
    match error {
        Error::InvalidArgument(message) | Error::Validation { message, .. } => {
            Status::invalid_argument(message)
        }
        Error::Unauthorized(message) => Status::unauthenticated(message),
        Error::Forbidden(message) => Status::permission_denied(message),
        Error::Unavailable(message) => Status::unavailable(message),
        Error::ResourceExhausted(message) => Status::resource_exhausted(message),
        Error::PayloadTooLarge(message) => Status::out_of_range(message),
        Error::Conflict(message) => Status::aborted(message),
        Error::NotFound(message) => Status::not_found(message),
        Error::Cancelled(message) => Status::cancelled(message),
        other => Status::internal(other.to_string()),
    }
}

pub fn status_to_error(status: Status) -> Error {
    let message = status.message().to_owned();
    match status.code() {
        Code::InvalidArgument => Error::invalid(message),
        Code::Unauthenticated => Error::unauthorized(message),
        Code::PermissionDenied => Error::forbidden(message),
        Code::Unavailable => Error::unavailable(message),
        Code::ResourceExhausted => Error::resource_exhausted(message),
        Code::OutOfRange => Error::payload_too_large(message),
        Code::Aborted => Error::conflict(message),
        Code::NotFound => Error::not_found(message),
        Code::Cancelled => Error::cancelled(message),
        _ => Error::internal(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_kind_survives_the_round_trip() {
        for (error, status) in [
            (Error::invalid("a"), 400),
            (Error::unauthorized("a"), 401),
            (Error::forbidden("a"), 403),
            (Error::not_found("a"), 404),
            (Error::conflict("a"), 409),
            (Error::payload_too_large("a"), 413),
            (Error::resource_exhausted("a"), 429),
            (Error::cancelled("a"), 499),
            (Error::unavailable("a"), 503),
            (Error::internal("a"), 500),
        ] {
            let back = status_to_error(error_to_status(error));
            assert_eq!(back.http_status_code(), status);
        }
    }

    #[test]
    fn the_message_is_preserved() {
        let status = error_to_status(Error::invalid("query could not be planned"));
        assert_eq!(status.code(), Code::InvalidArgument);
        assert!(
            status_to_error(status)
                .to_string()
                .contains("query could not be planned")
        );
    }
}
