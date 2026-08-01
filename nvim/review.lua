local review_id = vim.env.HERDR_COMMENTS_REVIEW_ID
local comments_bin = vim.env.HERDR_COMMENTS_BIN
local review_buffer = vim.api.nvim_get_current_buf()

if not review_id or review_id == "" or not comments_bin or comments_bin == "" then
  error("Herdr Comments review context is missing")
end

local function confirm()
  vim.fn.system({ comments_bin, "confirm-review", "--id", review_id })
  if vim.v.shell_error ~= 0 then
    error("Herdr Comments could not save this review")
  end
end

local function save_all()
  vim.api.nvim_buf_call(review_buffer, function()
    vim.cmd("write")
  end)
  vim.cmd("qa!")
end

vim.api.nvim_create_autocmd("BufWritePost", { buffer = review_buffer, callback = confirm })
vim.api.nvim_create_user_command("HerdrCommentsSaveAll", save_all, {})
vim.cmd([[
  cnoreabbrev <expr> wqa getcmdtype() ==# ':' && getcmdline() ==# 'wqa' ? 'HerdrCommentsSaveAll' : 'wqa'
]])
vim.keymap.set("n", "q", "<cmd>qa!<cr>", { buffer = review_buffer, silent = true })

vim.schedule(function()
  vim.notify(":wq, :wqa, or ZZ saves · q cancels")
end)
