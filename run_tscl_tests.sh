#!/bin/bash

# run all tests in this directory


for f in tests/compiler/*.tscl; do cargo run --release -- "$f"; done
