#!/bin/sh
# Create ephemeral experiment-only TLS material. Never install it on the host.
set -eu
case "$1" in
  /home/luoji/code/dashbeam-benchmark-20261111.*/tools) ;;
  *) echo 'expected isolated benchmark tools directory' >&2; exit 1 ;;
esac
cd "$1"
umask 077
mkdir tls
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
  -keyout tls/ca.key -out tls/ca.pem -days 2 -subj '/CN=DashBeam Benchmark CA' \
  -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign'
openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
  -keyout tls/relay.key -out tls/relay.csr -subj '/CN=relay.dashbeam.test'
openssl x509 -req -in tls/relay.csr -CA tls/ca.pem -CAkey tls/ca.key \
  -set_serial 1 -out tls/relay.pem -days 2 -extfile relay-cert.cnf
openssl verify -CAfile tls/ca.pem -verify_hostname relay.dashbeam.test tls/relay.pem
