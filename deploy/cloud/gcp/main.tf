# SPDX-License-Identifier: Apache-2.0
# Copyright (c) 2026 MoleSignal Authors
#
# Sends the Cloud Logging entries of one project to MoleSignal: a Log Router sink publishes the
# entries a filter selects to a Pub/Sub topic, and a subscription on the topic either pushes each
# message to MoleSignal (mode "push", for a "Google Cloud Pub/Sub push" connector) or waits for a
# "Google Cloud Pub/Sub pull" connector to read it (mode "pull", for larger volumes).
#
# It can also make the service accounts the pull connector and the Cloud Monitoring metrics
# connector sign in with. Their keys are made in the console or with
# `gcloud iam service-accounts keys create`, so no private key lands in the Terraform state.
#
#   terraform init
#   terraform apply -var project_id=my-project -var molesignal_url=https://molesignal.example.com \
#     -var push_token=<token>

terraform {
  required_version = ">= 1.5"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = ">= 5.0"
    }
  }
}

variable "project_id" {
  type        = string
  description = "The project whose logs are sent."
}

variable "mode" {
  type        = string
  default     = "push"
  description = "push: the subscription pushes to MoleSignal. pull: a pull connector reads it."

  validation {
    condition     = contains(["push", "pull"], var.mode)
    error_message = "mode is push or pull."
  }
}

variable "molesignal_url" {
  type        = string
  default     = ""
  description = "The address MoleSignal is reached at from Google Cloud, without a trailing slash. Needed for push."
}

variable "push_token" {
  type        = string
  default     = ""
  sensitive   = true
  description = "The token of the Google Cloud Pub/Sub push connector. Needed for push."
}

variable "log_filter" {
  type        = string
  default     = "severity >= DEFAULT"
  description = "Which entries the sink sends, in the Logging query language."
}

variable "metrics_reader" {
  type        = bool
  default     = false
  description = "Also make a service account with roles/monitoring.viewer for the Cloud Monitoring metrics connector."
}

provider "google" {
  project = var.project_id
}

resource "google_pubsub_topic" "logs" {
  name = "molesignal-logs"
}

resource "google_logging_project_sink" "logs" {
  name                   = "molesignal"
  destination            = "pubsub.googleapis.com/${google_pubsub_topic.logs.id}"
  filter                 = var.log_filter
  unique_writer_identity = true
}

resource "google_pubsub_topic_iam_member" "sink_publishes" {
  topic  = google_pubsub_topic.logs.id
  role   = "roles/pubsub.publisher"
  member = google_logging_project_sink.logs.writer_identity
}

resource "google_pubsub_subscription" "logs" {
  name  = "molesignal-${var.mode}"
  topic = google_pubsub_topic.logs.id

  # A pull is acknowledged once MoleSignal has written it; give it time to.
  ack_deadline_seconds       = 60
  message_retention_duration = "604800s"

  retry_policy {
    minimum_backoff = "10s"
    maximum_backoff = "600s"
  }

  dynamic "push_config" {
    for_each = var.mode == "push" ? [1] : []

    content {
      push_endpoint = "${var.molesignal_url}/api/v1/_gcp_pubsub?token=${var.push_token}"
    }
  }

  lifecycle {
    precondition {
      condition     = var.mode == "pull" || (startswith(var.molesignal_url, "https://") && length(var.push_token) >= 16)
      error_message = "push needs molesignal_url (https://) and push_token."
    }
  }
}

resource "google_service_account" "pull" {
  count        = var.mode == "pull" ? 1 : 0
  account_id   = "molesignal-pubsub"
  display_name = "MoleSignal Pub/Sub pull"
}

resource "google_pubsub_subscription_iam_member" "pull_consumes" {
  count        = var.mode == "pull" ? 1 : 0
  subscription = google_pubsub_subscription.logs.id
  role         = "roles/pubsub.subscriber"
  member       = "serviceAccount:${google_service_account.pull[0].email}"
}

resource "google_service_account" "metrics" {
  count        = var.metrics_reader ? 1 : 0
  account_id   = "molesignal-metrics"
  display_name = "MoleSignal Cloud Monitoring reader"
}

resource "google_project_iam_member" "metrics_reads" {
  count   = var.metrics_reader ? 1 : 0
  project = var.project_id
  role    = "roles/monitoring.viewer"
  member  = "serviceAccount:${google_service_account.metrics[0].email}"
}

output "subscription" {
  description = "The subscription a pull connector reads, as its subscription setting."
  value       = google_pubsub_subscription.logs.id
}

output "pull_service_account" {
  description = "Make a key for it and give it to the pull connector."
  value       = var.mode == "pull" ? google_service_account.pull[0].email : null
}

output "metrics_service_account" {
  description = "Make a key for it and give it to the Cloud Monitoring metrics connector."
  value       = var.metrics_reader ? google_service_account.metrics[0].email : null
}
