<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg">
  <img src="assets/logo.svg" alt="LanGame Server Manager" width="480">
</picture>

# LanGame Server Manager

[English](README.md) | [简体中文](README.zh-CN.md)

A Windows desktop application for installing, configuring, and managing game servers. Keep server files, settings, logs, and backups on your own machine, and manage multiple instances from one workspace.

## Main features

- **Install and update** dedicated-server packages through the supported game integrations.
- **Manage separate instances** with their own files, ports, and native game settings.
- **Operate servers** with start, stop, log inspection, and backup controls.
- **Monitor your host** through CPU, memory, disk, network, and instance overviews.
- **Manage players** with the queries and administrator actions supported by each game.
- **Work with LAN**, the optional AI assistant, using a configured local or remote model provider.

## Interface

### System overview

Host telemetry and instance status in one desktop workspace.

![System overview showing host telemetry and game-server instances](assets/system-overview.jpg)

### LAN assistant

Open LAN from the application header to ask questions about server operations.

![LAN assistant in the dark desktop interface](assets/lan-assistant.jpg)

*Screenshots use demonstration data. They do not show real servers, players, or workstation telemetry.*

## Game integrations

The current catalog contains **32 game integrations**, including:

- Palworld
- Minecraft
- Valheim
- ARK: Survival Ascended and ARK: Survival Evolved
- Don't Starve Together
- Rust
- 7 Days to Die
- Project Zomboid

Available settings, player queries, and administrator actions vary by game and server version. Each game also has its own hardware, storage, network, and dedicated-server requirements.

## Platform and language

LanGame Server Manager runs on **Windows** and provides **English and Simplified Chinese** interfaces. No LanGame account is required. LAN is optional; model-provider requirements depend on the selected service.

## About this repository

This repository presents the project and its interface. It does not contain application source code or installation packages.

Follow the repository for project information, or share a feature suggestion through [Issues](https://github.com/SZSLGJCOM/LanGame-Server-Manager/issues). Please keep credentials, private server details, and player information out of public posts.

Game names and trademarks belong to their respective owners. No dedicated-server binaries or proprietary game assets are distributed here.
