# manylinux x86_64 defaults GNUInstallDirs to lib64, while charls-sys searches
# its CMake install prefix under lib. Keep vendored native archives in the
# location expected by Cargo build scripts.
set(CMAKE_INSTALL_LIBDIR "lib" CACHE PATH "Native library install directory" FORCE)
