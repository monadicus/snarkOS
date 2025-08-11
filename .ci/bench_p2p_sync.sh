#!/bin/bash

###########################################################
# Measures a client syncing 1000 blocks from another client
###########################################################

network_id=1
min_height=240

# The number of clients that are syncing
num_clients=1

# Adjust this to show more/less log messages
log_filter="warn,snarkos_node_sync=debug"

max_wait=1800 # Wait for up to 30 minutes
poll_interval=1 # Check block heights every second

. ./.ci/utils.sh

network_name=$(get_network_name $network_id)
echo "Using network: $network_name (ID: $network_id)"

# Create log directory
log_dir=".logs-$(date +"%Y%m%d%H%M%S")"
mkdir -p "$log_dir"

# Array to store PIDs of all processes
declare -a PIDS

# Define a trap handler that cleans up all processes on exit.
function exit_handler() {
  shutdown "${PIDS[@]}"
}
trap exit_handler EXIT

# Define a trap handler that prints a message when an error occurs 
trap 'echo "⛔️ Error in $BASH_SOURCE at line $LINENO: \"$BASH_COMMAND\" failed (exit $?)"' ERR

# Shared flags betwen all nodes
common_flags=" --nodisplay --network $network_id --nocdn --dev-num-validators=40 \
  --no-dev-txs --log-filter=$log_filter"

# The client that has the ledger
# (runs on the first two cores)
taskset -c 0,1 snarkos start --dev 0 --client ${common_flags} \
  --logfile="$log_dir/client-0.log" &
PIDS[0]=$!
 
# Spawn the clients that will sync the ledger
# (running on the other two cores)
for ((client_index = 1; client_index <= num_clients; client_index++)); do
  prev_port=$((4130+client_index-1))

  # Ensure there are no old ledger files and the node syncs from scratch
  snarkos clean --dev $client_index --network $network_id || true

  taskset -c 2,3 snarkos start --dev $client_index --client ${common_flags} \
    --logfile="$log_dir/client-$client_index.log" \
    --peers=127.0.0.1:$prev_port &
  PIDS[client_index]=$!

  # Add 1-second delay between starting nodes to avoid hitting rate limits
  sleep 1
done

wait_for_nodes 0 $((num_clients+1))

# Check heights periodically with a timeout
total_wait=0
while (( total_wait < max_wait )); do
  if check_heights 0 $((num_clients+1)) $min_height "$network_name"; then
    # Use floating point division
    throughput=$(bc <<< "scale=2; $min_height/$total_wait")

    echo "🎉 Test passed!. Waited $total_wait for $min_height blocks. Throughput was $throughput blocks/s."

    # Append data to results file.
    printf "{ \"name\": \"p2p-sync\", \"unit\": \"blocks/s\", \"value\": %.3f, \"extra\": \"total_wait=%is\" },\n" \
       "$throughput" "$total_wait" | tee -a results.json
    shutdown "${PIDS[@]}"
    exit 0
  fi
  
  # Continue waiting
  sleep $poll_interval
  total_wait=$((total_wait+poll_interval))
  echo "Waited $total_wait seconds so far..."
done

echo "❌ Test failed! Clients did not sync within 30 minutes."

# Print logs for debugging
echo "Last 20 lines of client logs:"
for ((client_index = 0; client_index < num_clients; client_index++)); do
  echo "=== Client $client_index logs ==="
  tail -n 20 "$log_dir/client-$client_index.log"
done

shutdown "${PIDS[@]}"
exit 1
