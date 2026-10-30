# The icon set (assets/icons) and the image provider that colours and rasterises it.
file(GLOB icon_files CONFIGURE_DEPENDS ${PROJECT_SOURCE_DIR}/../assets/icons/*.svg)

qt_add_library(refgui_icons STATIC
	icon_provider.cpp
	icon_provider.h
)

# The files keep their names under the prefix: ":/icons/check-16.svg".
qt_add_resources(refgui_icons icon_set
	PREFIX /icons
	BASE ${PROJECT_SOURCE_DIR}/../assets/icons
	FILES ${icon_files}
)

target_include_directories(refgui_icons PUBLIC ${PROJECT_SOURCE_DIR}/src)
target_link_libraries(refgui_icons PUBLIC Qt6::Quick PRIVATE Qt6::Svg)
refgui_configure_target(refgui_icons)
