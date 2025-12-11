<div align="center">

  # rwiki

  **A blazing fast TUI Wikipedia viewer and explorer.**

  <a href="https://github.com/ezioalae/rwiki/issues">
    <img src="https://img.shields.io/github/issues/ezioalae/rwiki?style=flat-square&color=white&labelColor=white&logo=github&logoColor=black" alt="Issues">
  </a>
  <a href="https://github.com/ezioalae/rwiki/stargazers">
    <img src="https://img.shields.io/github/stars/ezioalae/rwiki?style=flat-square&color=white&labelColor=white&logo=star&logoColor=black" alt="Stars">
  </a>
  <a href="https://github.com/ezioalae/rwiki/blob/main/LICENSE">
    <img src="https://img.shields.io/github/license/ezioalae/rwiki?style=flat-square&color=white&labelColor=white&logo=open-source-initiative&logoColor=black" alt="License">
  </a>
  <a href="https://helix-editor.com/">
    <img src="https://img.shields.io/badge/Made%20with-Helix-white?style=flat-square&color=white&labelColor=white&logo=helix&logoColor=black" alt="Made with Helix">
  </a>

  <br />
  <br />

  <img src="https://media.discordapp.net/attachments/1440608190126489692/1448592893710303372/image.png?ex=693bd2c3&is=693a8143&hm=52a572faa7c2db6301f9736a75a3cc1b8f79541ef5ba31c8a0df504c0ecf77b8&=&format=webp&quality=lossless&width=1453&height=781" alt="rwiki Screenshot" width="100%">

</div>

<br />

## Overview

**rwiki** is a terminal-based Wikipedia reader designed for efficiency and focus. It allows you to browse, search, and read articles without leaving your keyboard or dealing with web trackers and ads. Built with **Rust**, it is lightweight and instant.

## Features

* **Blazing Fast:** Written in Rust for instant startup and low memory usage.
* **Distraction Free:** minimal TUI interface focused purely on content.
* **Keyboard Driven:** Navigate entirely without a mouse.
* **Search:** Quick fuzzy search to find articles instantly.

## Installation

### Prerequisites
Ensure you have [Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html) installed.

### Build from Source

```bash
git clone [https://github.com/ezioalae/rwiki.git](https://github.com/ezioalae/rwiki.git)
cd rwiki
cargo build --release
