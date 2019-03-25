#!/usr/bin/env bash

wget -qO- https://s3.amazonaws.com/kitsu-dev-dumps/2018-11-04.sql.gz | gunzip -c > sql_dump.sql