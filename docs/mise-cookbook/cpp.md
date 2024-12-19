# Mise + C++ Cookbook

Here are some tips on managing C++ projects with mise.

## A C++ Project with CMake

```toml [mise.toml]
min_version = "2024.9.5"

[env]
# Project information
PROJECT_NAME = "{{ config_root | basename }}"

# Build directory
BUILD_DIR = "{{ config_root }}/build"

[tools]
# Install CMake and make
cmake = "latest"
make = "latest"

[tasks.configure]
description = "Configure the project"
run = "mkdir -p $BUILD_DIR && cd $BUILD_DIR && cmake .."

[tasks.build]
description = "Build the project"
alias = "b"
run = "cd $BUILD_DIR && make"

[tasks.clean]
description = "Clean the build directory"
alias = "c"
run = "rm -rf $BUILD_DIR"

[tasks.run]
alias = "r"
description = "Run the application"
run = "$BUILD_DIR/bin/$PROJECT_NAME"

[tasks.info]
description = "Print project information"
run = '''
echo "Project: $PROJECT_NAME"
echo "Build Directory: $BUILD_DIR"
'''
```
