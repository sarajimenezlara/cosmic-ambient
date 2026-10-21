-- Catppuccin Mocha con la paleta del fondo puesto por cosmic-ambient, que la
-- escribe en ~/.local/state/cosmic-ambient/palette.lua; aquí se vigila el
-- archivo para recolorear en caliente. Para lazy.nvim: cópialo a lua/plugins/ y activa `neovim = true`
-- en ~/.config/cosmic-ambient/integraciones.toml.

local ruta = vim.fs.joinpath(vim.env.HOME, ".local/state/cosmic-ambient/palette.lua")

local function paleta()
  local ok, colores = pcall(dofile, ruta)
  return ok and type(colores) == "table" and colores or nil
end

local function vigilar()
  local w = vim.uv.new_fs_event()
  if not w then
    return
  end
  local pendiente = false
  w:start(vim.fs.dirname(ruta), {}, function(_, archivo)
    if archivo ~= "palette.lua" or pendiente then
      return
    end
    pendiente = true
    vim.defer_fn(function()
      pendiente = false
      local colores = paleta()
      if not colores then
        return
      end
      local opts = require("lazy.core.plugin").values(require("lazy.core.config").plugins.catppuccin, "opts", false)
      opts.color_overrides = { mocha = colores }
      require("catppuccin").setup(opts)
      if (vim.g.colors_name or ""):match("^catppuccin") then
        vim.cmd.colorscheme(vim.g.colors_name)
      end
    end, 150)
  end)
end

return {
  {
    "catppuccin/nvim",
    name = "catppuccin",
    opts = function(_, opts)
      local colores = paleta()
      if colores then
        opts.color_overrides = { mocha = colores }
      end
      if not vim.g.ambient_vigilando then
        vim.g.ambient_vigilando = true
        vigilar()
      end
    end,
  },
}
