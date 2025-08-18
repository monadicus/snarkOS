#!/bin/bash

if [[ -n "$TMUX" ]]; then
  echo "Detected nested tmux session. Try again after unsetting \$TMUX, e.g., using \`unset TMUX\` in bash."
  exit 1
fi

# Read the total number of validators from the user or use a default value of 4
read -p "Enter the total number of validators (default: 4): " total_validators
total_validators=${total_validators:-4}

# Read the total number of clients from the user or use a default value of 2
read -p "Enter the total number of clients (default: 2): " total_clients
total_clients=${total_clients:-2}

# Read the network ID from user or use a default value of 1
read -p "Enter the network ID (mainnet = 0, testnet = 1, canary = 2) (default: 1): " network_id
network_id=${network_id:-1}

# Ask the user if they want to run 'cargo install --locked --path .' or use a pre-installed binary
read -p "Do you want to run 'cargo install --locked --path .' to build the binary? (y/n, default: y): " build_binary
build_binary=${build_binary:-y}

# Ask the user whether to clear the existing ledger history
read -p "Do you want to clear the existing ledger history? (y/n, default: n): " clear_ledger
clear_ledger=${clear_ledger:-n}

# Log verbosity is set to 1 (DEBUG) by default.
verbosity=1

if [[ $build_binary == "y" ]]; then
  # Ask the user if they want to enable validator telemetry
  read -p "Do you want to enable validator telemetry? (y/n, default: y): " enable_telemetry
  enable_telemetry=${enable_telemetry:-y}

  # Ask the user for additional crate features (comma-separated)
  read -p "Enter crate features to enable (comma separated, default: test_network): " crate_features
  crate_features=${crate_features:-test_network}

  # Build command
  build_cmd="cargo install --locked --path ."

  # Add the telemetry feature if requested
  if [[ $enable_telemetry == "y" ]]; then
    build_cmd+=" --features telemetry"
  fi

  # Add any extra features if provided
  if [[ -n $crate_features ]]; then
    # If telemetry was also enabled, append with a comma separator
    if [[ $enable_telemetry == "y" ]]; then
      build_cmd+=",${crate_features}"
    else
      build_cmd+=" --features ${crate_features}"
    fi
  fi

  # Build command
  echo "Running build command: \"$build_cmd\""
  eval "$build_cmd" || exit 1
fi

# Clear the ledger logs for each validator if the user chooses to clear ledger
if [[ $clear_ledger == "y" ]]; then
  # Create an array to store background processes
  clean_processes=()

  for ((index = 0; index < $((total_validators + total_clients)); index++)); do
    # Run 'snarkos clean' for each node in the background
    snarkos clean --network $network_id --dev $index &

    # Store the process ID of the background task
    clean_processes+=($!)
  done

  # Wait for all 'snarkos clean' processes to finish
  for process_id in "${clean_processes[@]}"; do
    wait "$process_id"
  done
fi

# Create a timestamp-based directory for log files
log_dir=".logs-$(date +"%Y%m%d%H%M%S")"
mkdir -p "$log_dir"

# Create a new tmux session named "devnet"
tmux new-session -d -s "devnet" -n "validator-0"
if [[ $? -ne 0 ]]; then
  echo "Failed to create new TMUX session."
  exit 1
fi

# Get the tmux's base-index for windows
# we have to create all windows with index offset by this much
index_offset="$(tmux show-option -gv base-index)"
if [ -z "$index_offset" ]; then
  index_offset=0
fi

# Generate validator indices from 0 to (total_validators - 1)
validator_indices=($(seq 0 $((total_validators - 1))))

# Loop through the list of validator indices and create a new window for each
for validator_index in "${validator_indices[@]}"; do
  # Generate a unique and incrementing log file name based on the validator indexi
  name="validator-$validator_index"
  log_file="$log_dir/$name.log"
  window_index=$((validator_index + index_offset))
  metrics_port=$((validator_index + 9000))

  if [ "$validator_index" -ne 0 ]; then
    # We don't need to create a window for the first validator because the tmux session already starts with one window.
    tmux new-window -t "devnet:$window_index" -n $name
  fi

  # Send the command to start the validator to the new window and capture output to the log file
  tmux send-keys -t "devnet:$window_index" "snarkos start --nodisplay --network $network_id --dev $validator_index --allow-external-peers --dev-num-validators $total_validators --validator --logfile $log_file --verbosity $verbosity --metrics --metrics-ip=0.0.0.0:$metrics_port --no-dev-txs" C-m
done

if [ "$total_clients" -ne 0 ]; then
  # Generate client indices from 0 to (total_clients - 1)
  client_indices=($(seq 0 $((total_clients - 1))))

  # Loop through the list of client indices and create a new window for each
  for client_index in "${client_indices[@]}"; do
    # Generate a unique and incrementing log file name based on the client index
    name="client-$client_index"
    log_file="$log_dir/$name.log"

    window_index=$((client_index + total_validators + index_offset))

    # Create a new window with a unique name
    tmux new-window -t "devnet:$window_index" -n $name

    # Send the command to start the client to the new window and capture output to the log file
    tmux send-keys -t "devnet:$window_index" "snarkos start --nodisplay --network $network_id --dev $window_index --dev-num-validators $total_validators --client --logfile $log_file  --verbosity $verbosity" C-m
  done
fi

# Attach to the tmux session to view and interact with the windows
tmux attach-session -t "devnet"
