local review_id = vim.env.HERDR_COMMENTS_REVIEW_ID
local comments_bin = vim.env.HERDR_COMMENTS_BIN

if not review_id or review_id == "" or not comments_bin or comments_bin == "" then
  error("Herdr Comments review context is missing")
end

local function confirm()
  vim.fn.system({ comments_bin, "confirm-review", "--id", review_id })
  if vim.v.shell_error ~= 0 then
    error("Herdr Comments could not save this review")
  end
end

vim.api.nvim_create_autocmd("BufWritePost", { buffer = 0, callback = confirm })
vim.keymap.set("n", "q", "<cmd>qa!<cr>", { buffer = 0, silent = true })

vim.schedule(function()
  vim.notify(":wq or ZZ saves · q cancels")
end)
