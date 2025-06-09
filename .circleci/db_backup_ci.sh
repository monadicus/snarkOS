#!/bin/bash

# Network parameters
total_validators=4
network_id=0
network_name="mainnet"

# Stopping conditions
checkpoint_height=3
rollback_height=10
num_checkpoints=0
remaining_checkpoints=2

# Use fixed JWT values in order to be able to create checkpoints
jwt_secret="ZGJjaGVja3BvaW50dGVzdA=="
jwt_ts=1749116345
jwt[0]="eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGVvMXJoZ2R1NzdoZ3lxZDN4amo4dWN1M2pqOXIya3J3ejZtbnp5ZDgwZ25jcjVmeGN3bGg1cnN2enA5cHgiLCJpYXQiOjE3NDkxMTYzNDUsImV4cCI6MjA2NDQ3NjM0NX0.qm2idfIm4ZTFOsyT19lH9pcWzzAtP5mbymkN4oL6_sc"
jwt[1]="eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGVvMXMzd3M1dHJhODdmanljbmpyd3NqY3JudzJxeHI4amZxcWR1Z25mMHh6cXF3MjlxOW01cHFlbTJ1NHQiLCJpYXQiOjE3NDkxMTYzNDUsImV4cCI6MjA2NDQ3NjM0NX0.4efs4qWJuG0Lm2CxrLMIKrrbJiGD-XNqHlk_AUaXOBo"
jwt[2]="eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGVvMWFzaHl1OTZ0andlNjN1MGd0bm52OHo1bGhhcGR1NGw1cGpzbDJraGE3ZnY3aHZ6MmVxeHM1ZHowcmciLCJpYXQiOjE3NDkxMTYzNDUsImV4cCI6MjA2NDQ3NjM0NX0.zxO1ajmQ0Wqr1gg4NuRzH4i_hiUBt7_fP9WP3KHbp4c"
jwt[3]="eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGVvMTJ1eDNnZGF1Y2swdjYwd2VzdGdjcHFqN3Y4cnJjcjN2MzQ2ZTRqdHEwNHE3a2t0MjJjenNoODA4djIiLCJpYXQiOjE3NDkxMTYzNDUsImV4cCI6MjA2NDQ3NjM0NX0.bJZ-fcrJwaI5YdPXDQ1nySV-jmxeABQCSvL1Ag9CSpo"

# Array to store PIDs of all processes
declare -a PIDS

# Start all validator nodes in the background
for ((validator_index = 0; validator_index < $total_validators; validator_index++)); do
  snarkos start --nodisplay --network $network_id --dev $validator_index --dev-num-validators $total_validators --validator --jwt-secret $jwt_secret --jwt-timestamp $jwt_ts &
  PIDS[$validator_index]=$!
  echo "Started validator $validator_index with PID ${PIDS[$validator_index]}"
  # Add 1-second delay between starting nodes to avoid hitting rate limits
  sleep 1
done

# Function to check block heights; the 1st parameter is the desired height
check_heights() {
  echo "Checking block heights on all nodes..."
  num_done=0
  for ((node_index = 0; node_index < $total_validators; node_index++)); do
    port=$((3030 + node_index))
    height=$(curl -s "http://127.0.0.1:$port/$network_name/block/height/latest" || echo "0")

    # Track highest height for reporting
    if [[ "$height" =~ ^[0-9]+$ ]] && [ $height -ge $1 ]; then
      num_done=$((num_done + 1))
    fi
  done

  if [ $num_done -eq $total_validators ]; then
    echo "All nodes reached the height of $1"
    return 0
  else
    return 1
  fi
}

# Create database checkpoints
create_checkpoints() {
  for ((node_index = 0; node_index < $total_validators; node_index++)); do
    port=$((3030 + node_index))
    suffix="${node_index}_$1"
    result=$(curl -s -X "POST" -H "Authorization: Bearer ${jwt[node_index]}" "http://127.0.0.1:$port/$network_name/db_backup?path=/tmp/checkpoint_$suffix" || echo "fail")

    # Track highest height for reporting
    if [ "$result" = "fail" ]; then
      return 1
    fi
  done

  echo "All nodes created a checkpoint"
  return 0
}

# Wait for 15 seconds to let the network start
echo "Waiting 15 seconds for network to start up..."
sleep 15

# Check heights periodically with a timeout
total_wait=0
checkpoint_created=false
while [ $total_wait -lt 300 ]; do  # 5 minutes max
  # Apply short-circuiting
  if [[ $checkpoint_created = true ]] || check_heights "$checkpoint_height"; then
    if [[ $checkpoint_created = false ]]; then
      # Create checkpoints at the specified height
      create_checkpoints $num_checkpoints
      checkpoint_created=true
      checkpoint_height=$((checkpoint_height+2))
      num_checkpoints=$((num_checkpoints+1))

      echo "num_checkpoints: $num_checkpoints"
      sleep 2
    fi

    # Wait until the specified rollback height is reached
    if check_heights "$rollback_height"; then
      echo "All nodes reached rollback height."

      checkpoint_created=false

      # Gracefully shut down the validators
      for pid in "${PIDS[@]}"; do
        kill -15 $pid 2>/dev/null || true
      done
      # Wait until the shutdown concludes.
      sleep 5

      for ((validator_index = 0; validator_index < $total_validators; validator_index++)); do
        # Remove the original ledger
        if (( num_checkpoints == 1 )); then
          snarkos clean --network $network_id --dev $validator_index
        else
          suffix="${validator_index}_$((num_checkpoints-2))"
          snarkos clean --network $network_id --dev $validator_index --path=/tmp/checkpoint_$suffix
        fi
        # Wait until the cleanup concludes
        sleep 1
        # Restart using the checkpoint
        suffix="${validator_index}_$((num_checkpoints-1))"
        snarkos start --nodisplay --network $network_id --dev $validator_index --dev-num-validators $total_validators --validator --jwt-secret $jwt_secret --jwt-timestamp $jwt_ts --storage /tmp/checkpoint_$suffix &
        PIDS[$validator_index]=$!
        echo "Restarted validator $validator_index with PID ${PIDS[$validator_index]}"
        # Add 1-second delay between starting nodes to avoid hitting rate limits
        sleep 1

        port=$((3030 + validator_index))
        height=$(curl -s "http://127.0.0.1:$port/$network_name/block/height/latest" || echo "0")
        echo "Node height after restart: $height"

        # Ensure that the height is below the rollback height
        if [[ "$height" =~ ^[0-9]+$ ]] && (( height >= rollback_height )) && (( height < checkpoint_height )); then
          echo "❌ Test failed!"
          exit 1
        fi
      done

      if (( remaining_checkpoints == 0 )); then
        echo "SUCCESS!"

        # Cleanup: kill all processes
        for pid in "${PIDS[@]}"; do
          kill -9 $pid 2>/dev/null || true
        done

        exit 0
      fi

      remaining_checkpoints=$((remaining_checkpoints-1))

    fi
  fi

  # Continue waiting
  sleep 3
  total_wait=$((total_wait + 3))
  echo "Waited $total_wait seconds so far..."
done

# The main loop has expired by now
echo "❌ Test failed!"

# Cleanup: kill all processes
for pid in "${PIDS[@]}"; do
  kill -9 $pid 2>/dev/null || true
done

exit 1
