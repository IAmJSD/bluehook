#!/usr/bin/env python3

import os

image = os.environ["IMAGE"]
file = open("deployment.yml", "r").read()
open("deployment.out.yml", "w").write(file.replace("{image}", image))
