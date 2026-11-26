//! Check the limited declarations used by crate-root data recovery.
use syn::{
    Attribute, Item, UseTree,
    visit::{self, Visit},
};

pub(crate) fn ordinary_root(file: &syn::File) -> Result<(), String> {
    let mut unix = false;
    for attribute in &file.attrs {
        if attribute.path().is_ident("doc") {
            continue;
        }
        if attribute.path().is_ident("cfg")
            && attribute
                .parse_args::<syn::Ident>()
                .is_ok_and(|value| value == "unix")
            && !unix
        {
            unix = true;
        } else {
            return Err("unsupported conditional or expanded crate-root data scope".into());
        }
    }
    Ok(())
}

fn unusual_macro_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("macro_use"))
}

pub(crate) fn name(ident: &syn::Ident, names: &[&str]) -> bool {
    let ident = ident.to_string();
    names.contains(&ident.strip_prefix("r#").unwrap_or(&ident))
}

pub(crate) fn imports_reserved(
    tree: &UseTree,
    parent: Option<&syn::Ident>,
    names: &[&str],
) -> bool {
    match tree {
        UseTree::Path(next) => imports_reserved(&next.tree, Some(&next.ident), names),
        UseTree::Name(item) if item.ident == "self" => {
            parent.is_none_or(|parent| name(parent, names))
        }
        UseTree::Name(item) => name(&item.ident, names),
        UseTree::Rename(item) => name(&item.rename, names),
        UseTree::Group(group) => group
            .items
            .iter()
            .any(|item| imports_reserved(item, parent, names)),
        UseTree::Glob(_) => true,
    }
}

pub(crate) fn bindings(file: &syn::File, selected: &[&str]) -> Result<(), String> {
    struct Mention<'a> {
        names: &'a [&'a str],
        found: bool,
    }

    impl<'ast> Visit<'ast> for Mention<'_> {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            self.found |= name(ident, self.names);
        }
    }

    struct Scan<'a> {
        names: &'a [&'a str],
        forbidden: bool,
    }

    impl Scan<'_> {
        fn ty(&mut self, ty: &syn::Type) {
            let mut mention = Mention {
                names: self.names,
                found: false,
            };
            mention.visit_type(ty);
            self.forbidden |= mention.found;
        }
    }

    impl<'ast> Visit<'ast> for Scan<'_> {
        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            self.ty(&item.self_ty);
            let mut mention = Mention {
                names: self.names,
                found: false,
            };
            if let Some((_, trait_path, _)) = &item.trait_ {
                mention.visit_path(trait_path);
            }
            mention.visit_generics(&item.generics);
            self.forbidden |= mention.found;
            visit::visit_item_impl(self, item);
        }

        fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
            self.ty(&item.ty);
        }
        fn visit_impl_item_type(&mut self, item: &'ast syn::ImplItemType) {
            self.ty(&item.ty);
        }
        fn visit_trait_item_type(&mut self, item: &'ast syn::TraitItemType) {
            if let Some((_, ty)) = &item.default {
                self.ty(ty);
            }
        }
        fn visit_use_rename(&mut self, item: &'ast syn::UseRename) {
            self.forbidden |= name(&item.ident, self.names);
        }
        fn visit_item_macro(&mut self, _: &'ast syn::ItemMacro) {
            self.forbidden = true;
        }
        fn visit_item(&mut self, item: &'ast Item) {
            match item {
                Item::ExternCrate(item) => self.forbidden |= unusual_macro_attr(&item.attrs),
                Item::Mod(item) => self.forbidden |= unusual_macro_attr(&item.attrs),
                _ => {}
            }
            visit::visit_item(self, item);
        }
    }

    let mut scan = Scan {
        names: selected,
        forbidden: false,
    };
    scan.visit_file(file);
    if scan.forbidden {
        return Err("unsupported macros, aliases or impls in crate-root data scope".into());
    }
    Ok(())
}
