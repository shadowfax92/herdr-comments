local review_id = vim.env.HERDR_COMMENTS_REVIEW_ID
local comments_bin = vim.env.HERDR_COMMENTS_BIN

if not review_id or review_id == "" or not comments_bin or comments_bin == "" then
  error("Herdr Comments review context is missing")
end

vim.api.nvim_create_autocmd("BufWritePost", {
  buffer = 0,
  callback = function()
    vim.fn.system({ comments_bin, "confirm-review", "--id", review_id })
    if vim.v.shell_error ~= 0 then
      vim.notify("Herdr Comments could not confirm this review", vim.log.levels.ERROR)
    end
  end,
})

vim.schedule(function()
  vim.notify("Write and quit to paste; quit without writing to cancel")
end)
