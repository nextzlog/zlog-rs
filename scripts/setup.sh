set -eu
if [ "$RUNNER_OS" = Linux ]; then
  sudo apt-get update
  sudo apt-get install -y libasound2-dev mesa-vulkan-drivers libgl1-mesa-dri libegl1
fi
