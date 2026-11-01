local lito = require("@lito")
local qt = require("@lito.qt")

local plugin = lito.target({ kind = "lib", name = "qextraplugin" })
local qt6 = lito.external_dependency(plugin, "qt6")

qt.moc({
  target = plugin,
  qt = qt6,
  files = {
    {
      source = "plugin.hpp",
      mode = "separate",
      output = "moc_plugin.cpp",
      plugin_metadata = { uri = "QExtra" },
    },
  },
})
