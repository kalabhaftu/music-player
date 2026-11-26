import { syncSelect } from "./selects.js";
// Retain rows, artwork, focus, and scroll position during a background refresh.
export function patchMarkup(root, markup) {
  const template=document.createElement("template");template.innerHTML=markup;
  const scroller=root.closest(".content"),scrollTop=scroller?.scrollTop;
  const key=node=>node.nodeType===1?(node.id||node.getAttribute("data-queue-index")||node.getAttribute("data-track")||null):null;
  function patch(parent,source){
    const keyed=new Map();
    for(const node of parent.childNodes){const id=key(node);if(id!==null){if(!keyed.has(id))keyed.set(id,[]);keyed.get(id).push(node);}}
    let cursor=parent.firstChild;
    for(const wanted of [...source.childNodes]){
      const id=key(wanted);
      let actual=id!==null?keyed.get(id)?.shift():cursor;
      if(wanted.nodeName==="SELECT"){
        const select=[...parent.children].find(node=>node.matches(".select-control")&&(wanted.id?node.querySelector("select")?.id===wanted.id:node.querySelector("select")?.dataset.setting===wanted.dataset.setting))?.querySelector("select");
        if(select){const wrapper=select.parentElement;if(wrapper!==cursor)parent.insertBefore(wrapper,cursor);select.replaceChildren(...[...wanted.childNodes].map(node=>node.cloneNode(true)));select.value=wanted.value;select.disabled=wanted.disabled;syncSelect(select);cursor=wrapper.nextSibling;continue;}
      }
      if(actual&&(actual.nodeType!==wanted.nodeType||actual.nodeName!==wanted.nodeName||key(actual)!==id))actual=null;
      if(!actual){actual=wanted.cloneNode(true);parent.insertBefore(actual,cursor);}
      else {
        if(actual!==cursor)parent.insertBefore(actual,cursor);
        if(actual.nodeType===1){
          // Enhanced selects own their button and popup; keep them mounted.
          if(actual.matches(".select-control")){cursor=actual.nextSibling;continue;}
          for(const attribute of [...actual.attributes])if(!wanted.hasAttribute(attribute.name))actual.removeAttribute(attribute.name);
          for(const attribute of wanted.attributes)if(actual.getAttribute(attribute.name)!==attribute.value)actual.setAttribute(attribute.name,attribute.value);
          if(actual.tagName!=="INPUT"&&actual.tagName!=="TEXTAREA")patch(actual,wanted);
        }else if(actual.nodeValue!==wanted.nodeValue)actual.nodeValue=wanted.nodeValue;
      }
      cursor=actual.nextSibling;
    }
    while(cursor){const next=cursor.nextSibling;cursor.remove();cursor=next;}
  }
  patch(root,template.content);
  if(scroller)scroller.scrollTop=scrollTop;
}
