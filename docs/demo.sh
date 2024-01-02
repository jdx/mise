#!/usr/bin/env bash
set -e

cd
rm -rf ~/myproj/.* ~/.mise/installs ~/.config/mise
PATH="$HOME/.cargo/bin:$PATH" vhs <~/src/mise/docs/demo.tape
