local review_id = vim.env.HERDR_COMMENTS_REVIEW_ID
local comments_bin = vim.env.HERDR_COMMENTS_BIN

if not review_id or review_id == "" or not comments_bin or comments_bin == "" then
  error("Herdr Comments review context is missing")
end

local function submit()
  local wrote, write_error = pcall(vim.cmd.write)
  if not wrote then
    vim.notify(write_error, vim.log.levels.ERROR)
    return
  end
  vim.fn.system({ comments_bin, "confirm-review", "--id", review_id })
  if vim.v.shell_error ~= 0 then
    vim.notify("Herdr Comments could not confirm this review", vim.log.levels.ERROR)
    return
  end
  vim.cmd("qa")
end

vim.api.nvim_create_user_command("HerdrCommentsSubmit", submit, {})
vim.cmd([[
  cnoreabbrev <expr> wq getcmdtype() ==# ':' && getcmdline() ==# 'wq' ? 'HerdrCommentsSubmit' : 'wq'
]])
vim.keymap.set("n", "q", "<cmd>qa!<cr>", { buffer = 0, silent = true })
vim.keymap.set("n", "ZZ", submit, { buffer = 0, silent = true })

vim.schedule(function()
  vim.notify(":wq or ZZ pastes · q cancels")
end)
