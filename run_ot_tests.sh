#!/bin/bash

# run all tests in this directory


for f in tests/compiler/*.ot; do cargo run --release -- "$f"; done
