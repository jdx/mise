#!/usr/bin/env bash
set -e

cd
rm -rf ~/myproj/.* ~/.rtx/installs ~/.config/rtx
PATH="$HOME/.cargo/bin:$PATH" vhs < ~/src/rtx/docs/demo.tape
