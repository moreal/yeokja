-- Lift LaTeXML's page-layout wrappers so Pandoc can split EPUB content at
-- chapter-level headings. Semantic inner divisions remain untouched.

local function has_class(classes, expected)
  for _, class in ipairs(classes) do
    if class == expected then
      return true
    end
  end
  return false
end

function Div(div)
  if div.attributes.role == "main"
    or has_class(div.classes, "ltx_page_content")
    or has_class(div.classes, "ltx_chapter")
    or has_class(div.classes, "ltx_part")
    or has_class(div.classes, "ltx_appendix")
    or has_class(div.classes, "ltx_bibliography")
  then
    return div.content
  end
end
