cmake_minimum_required(VERSION 3.16)
project(pam_google VERSION 0.1.0 LANGUAGES C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# Find required libraries
find_package(PkgConfig REQUIRED)
pkg_check_modules(LIBSECRET REQUIRED libsecret-1)
pkg_check_modules(JSON_C REQUIRED json-c)

find_library(PAM_LIBRARY pam REQUIRED)
find_library(CURL_LIBRARY curl REQUIRED)
find_path(PAM_INCLUDE_DIR security/pam_modules.h REQUIRED)
find_path(CURL_INCLUDE_DIR curl/curl.h REQUIRED)

# pam_google shared library
add_library(pam_google SHARED
    src/pam_google.c
    src/token_refresh.c
    src/account_lookup.c
    src/offline_auth.c
    src/keyring.c
)

target_include_directories(pam_google PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${PAM_INCLUDE_DIR}
    ${CURL_INCLUDE_DIR}
    ${LIBSECRET_INCLUDE_DIRS}
    ${JSON_C_INCLUDE_DIRS}
)

target_link_libraries(pam_google PRIVATE
    ${PAM_LIBRARY}
    ${CURL_LIBRARY}
    ${LIBSECRET_LIBRARIES}
    ${JSON_C_LIBRARIES}
    crypt   # for bcrypt in offline_auth
)

# Remove 'lib' prefix — PAM modules are named pam_*.so
set_target_properties(pam_google PROPERTIES
    PREFIX ""
    OUTPUT_NAME "pam_google"
)

# Install to /usr/lib/security/
install(TARGETS pam_google
    LIBRARY DESTINATION lib/security
)
