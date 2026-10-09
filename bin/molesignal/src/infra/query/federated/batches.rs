// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 联邦分片的编码。

use crate::domain::stream::StreamType;

pub(super) fn stream_type_to_proto(stream_type: StreamType) -> String {
    stream_type.external_name().to_owned()
}
