#!/usr/bin/env bash

# generate a genesis block and initial ledger

NAME="$(date +%Y%m%d_%H%M%S)"
SNARKOS="$(pwd)/target/release/snarkos"

mkdir -p tests/$NAME

GENESIS=tests/$NAME/genesis.block
COMMITTEE=tests/$NAME/committee.json
TRANSACTIONS=tests/$NAME/tx.json
LEDGER=tests/$NAME/ledger

echo "Creating test $NAME"

pk() { cat $COMMITTEE | jq "[.[][0]][$1]" -r; }
addr() { cat $COMMITTEE | jq "(. | keys)[$1]" -r; }

# generate the genesis block
$SNARKOS genesis --committee-size 3 --committee-file $COMMITTEE --output $GENESIS --bonded-balance 10000000000000
GENESIS_PK=$(pk 0)

# setup the ledger
$SNARKOS ledger init --genesis $GENESIS --output $LEDGER

echo "Start a validator with \`scripts/validator.sh 0\`"
echo "Broadcast some transactions with \`scripts/tx_cannon.sh\`"
