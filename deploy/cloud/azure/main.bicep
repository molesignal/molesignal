// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors
//
// An event hub for Azure Monitor to export to and a MoleSignal Kafka source to read: an Event
// Hubs namespace on the Standard tier (Basic has no Kafka endpoint), an event hub, a consumer
// group, a rule diagnostic settings send with, and a rule MoleSignal listens with.
//
// Diagnostic settings and data collection rules are then pointed at the event hub (see
// README.md), and a Kafka source with `messageFormat: azure_records` reads it with the outputs.
//
//   az deployment group create --resource-group <group> --template-file main.bicep

@description('Where the namespace is made; diagnostic settings must export to one in their resources\' region.')
param location string = resourceGroup().location

@description('The Event Hubs namespace, unique across Azure.')
param namespaceName string = 'molesignal-${uniqueString(resourceGroup().id)}'

@description('Throughput units of the namespace.')
@minValue(1)
@maxValue(40)
param capacity int = 1

@description('How long the hub keeps events, in days.')
@minValue(1)
@maxValue(7)
param retentionDays int = 1

resource namespace 'Microsoft.EventHub/namespaces@2024-01-01' = {
  name: namespaceName
  location: location
  sku: {
    name: 'Standard'
    tier: 'Standard'
    capacity: capacity
  }
  properties: {
    kafkaEnabled: true
    minimumTlsVersion: '1.2'
  }
}

resource hub 'Microsoft.EventHub/namespaces/eventhubs@2024-01-01' = {
  parent: namespace
  name: 'molesignal'
  properties: {
    partitionCount: 4
    messageRetentionInDays: retentionDays
  }
}

resource consumerGroup 'Microsoft.EventHub/namespaces/eventhubs/consumergroups@2024-01-01' = {
  parent: hub
  name: 'molesignal'
}

// Diagnostic settings need Manage, Send and Listen on a namespace rule.
resource exportRule 'Microsoft.EventHub/namespaces/authorizationRules@2024-01-01' = {
  parent: namespace
  name: 'azure-monitor-export'
  properties: {
    rights: [
      'Manage'
      'Send'
      'Listen'
    ]
  }
}

// MoleSignal only reads.
resource readRule 'Microsoft.EventHub/namespaces/eventhubs/authorizationRules@2024-01-01' = {
  parent: hub
  name: 'molesignal-read'
  properties: {
    rights: [
      'Listen'
    ]
  }
}

@description('The rule diagnostic settings export with (`--event-hub-rule`).')
output exportRuleId string = exportRule.id

@description('The Kafka source\'s `brokers`.')
output brokers string = '${namespace.name}.servicebus.windows.net:9093'

@description('The Kafka source\'s `topic`.')
output topic string = hub.name

@description('The Kafka source\'s SASL password, with `$ConnectionString` as the user name.')
#disable-next-line outputs-should-not-contain-secrets
output connectionString string = readRule.listKeys().primaryConnectionString
