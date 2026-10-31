# Adapted from an early version of qutie by LightBlueCube
# SPDX-License-Identifier: GPL-3.0-or-later
# Shared compile settings for every ref-gui target (GCC, Clang and MSVC).

option(REFGUI_SANITIZE "Build with AddressSanitizer and UndefinedBehaviorSanitizer" OFF)

function(refgui_configure_target target)
	if(MSVC)
		# /permissive- and /Zc:__cplusplus make MSVC follow the standard (Qt needs the latter
		# to see the real C++ version); /utf-8 reads sources and string literals as UTF-8.
		# CMake passes the Qt and dependency include folders as -external:I; /external:W0 keeps
		# MSVC from warning about code in them (C4702 in qmetatype.h, hit by the generated QML
		# cache sources), which warnings-as-errors would turn into a failed build. Warnings from
		# template instantiations are not always held back, so C4702 is also off: MSVC reports
		# code after a constant `if`/`return` in templates as unreachable.
		target_compile_options(${target} PRIVATE /W4 /external:W0 /wd4702 /permissive- /Zc:__cplusplus /utf-8 /EHsc)
	else()
		target_compile_options(${target} PRIVATE
			-Wall
			-Wextra
			-Wpedantic
			-Wshadow
			-Wnon-virtual-dtor
			-Woverloaded-virtual
			-Wold-style-cast
			-Wcast-qual
			-Wconversion
			-Wimplicit-fallthrough
		)
	endif()
	# Override locally with `cmake --compile-no-warning-as-error`.
	set_target_properties(${target} PROPERTIES COMPILE_WARNING_AS_ERROR ON)

	target_compile_definitions(${target} PRIVATE
		# Disables Qt APIs deemed deprecated or error-prone up to 6.10 (see qtconfigmacros.h).
		QT_ENABLE_STRICT_MODE_UP_TO=0x060A00
		# Every char* -> QString conversion must state its encoding.
		QT_NO_CAST_FROM_ASCII
	)

	if(REFGUI_SANITIZE AND NOT MSVC)
		target_compile_options(${target} PRIVATE -fsanitize=address,undefined -fno-omit-frame-pointer)
		target_link_options(${target} PRIVATE -fsanitize=address,undefined)
	endif()
endfunction()
