---
description: "Install CMake with mise and define configure, build and clean tasks for a C++ project."
---

# C++ and CMake

Pin CMake for a C++ project and run its configure, build and clean steps as
tasks. A C or C++ compiler must already be installed, for example through your
operating system's development tools; installing CMake does not install a
compiler.

## Build a CMake project {#a-c-project-with-cmake}

The tasks use CMake's build interface, so they work with any generator, such as
Make or Ninja:

```toml [mise.toml]
[tools]
cmake = "4"

[tasks.configure]
description = "Configure the CMake build"
run = "cmake -S . -B build"

[tasks.build]
description = "Build the project"
alias = "b"
depends = ["configure"]
run = "cmake --build build"

[tasks.clean]
description = "Clean compiled targets and keep the CMake configuration"
alias = "c"
run = "cmake --build build --target clean"
```

For a runnable example, add a `CMakeLists.txt` and a source file next to
`mise.toml`. `cmake_minimum_required` states the oldest CMake the project
supports; the `[tools]` entry picks the one you build with:

```cmake [CMakeLists.txt]
cmake_minimum_required(VERSION 3.20)
project(hello LANGUAGES CXX)
add_executable(hello main.cpp)
```

```cpp [main.cpp]
#include <iostream>

int main() {
    std::cout << "Hello from CMake\n";
}
```

Run `mise run build`, then `./build/hello`. The build task configures the
`build` directory before it compiles. Add `build/` to `.gitignore`.

With a single-configuration generator such as Unix Makefiles, the program is
`build/hello`. Multi-configuration generators, such as Visual Studio or Ninja
Multi-Config, put it in a configuration subdirectory; build one configuration
with `mise run build -- --config Debug` and look in that generator's output
path.

After the first build, `mise run clean` removes compiled targets with the
generator's clean target and keeps the build configuration. See
[CMake's command-line reference](https://cmake.org/cmake/help/latest/manual/cmake.1.html)
for generator selection and build options.
