/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2023-2025 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

#ifndef XML_H
#define XML_H

#include <iostream>
#include <memory>
#include <string>
#include <vector>
#include "nzbget_rs.h"

namespace Xml
{
struct XmlNode
{
	std::string name;
	std::string text;
	bool isText = false;
	XmlNode* parent = nullptr;
	std::vector<std::unique_ptr<XmlNode>> children;
};
using XmlNodePtr = XmlNode*;

inline void AppendEscaped(std::string& out, const std::string& raw)
{
	NzbgetRsBuf buf = nzbget_rs_xml_encode(raw.c_str());
	if (buf.data && buf.len > 0)
	{
		out.append(buf.data, buf.len);
	}
	nzbget_rs_free(buf);
}

inline void SerializeNode(const XmlNode* node, std::string& out)
{
	if (!node)
	{
		return;
	}
	out.push_back('<');
	out.append(node->name);
	out.push_back('>');
	if (node->isText)
	{
		AppendEscaped(out, node->text);
	}
	else
	{
		for (const auto& child : node->children)
		{
			SerializeNode(child.get(), out);
		}
	}
	out.append("</");
	out.append(node->name);
	out.push_back('>');
}

inline XmlNodePtr AddChild(XmlNodePtr parent, std::unique_ptr<XmlNode> child)
{
	child->parent = parent;
	XmlNodePtr ptr = child.get();
	parent->children.push_back(std::move(child));
	return ptr;
}

inline std::string Serialize(const XmlNodePtr rootNode)
{
	std::string result;
	SerializeNode(rootNode, result);
	return result;
}

inline void AddNewNode(XmlNodePtr rootNode, const char* name, const char* type, const char* value)
{
	auto memberNode = std::make_unique<XmlNode>();
	memberNode->name = "member";

	auto nameNode = std::make_unique<XmlNode>();
	nameNode->name = "name";
	nameNode->text = name ? name : "";
	nameNode->isText = true;
	AddChild(memberNode.get(), std::move(nameNode));

	auto valueNode = std::make_unique<XmlNode>();
	valueNode->name = "value";

	auto typeNode = std::make_unique<XmlNode>();
	typeNode->name = type ? type : "string";
	typeNode->text = value ? value : "";
	typeNode->isText = true;
	AddChild(valueNode.get(), std::move(typeNode));

	AddChild(memberNode.get(), std::move(valueNode));
	AddChild(rootNode, std::move(memberNode));
}

inline XmlNodePtr CreateStructNode()
{
	auto* valueNode = new XmlNode();
	valueNode->name = "value";
	auto structNode = std::make_unique<XmlNode>();
	structNode->name = "struct";
	return AddChild(valueNode, std::move(structNode));
}

inline void AddArrayNode(XmlNodePtr structNode, const char* name, const std::vector<XmlNodePtr>& children)
{
	auto memberNode = std::make_unique<XmlNode>();
	memberNode->name = "member";

	auto nameNode = std::make_unique<XmlNode>();
	nameNode->name = "name";
	nameNode->text = name ? name : "";
	nameNode->isText = true;
	AddChild(memberNode.get(), std::move(nameNode));

	auto valueNode = std::make_unique<XmlNode>();
	valueNode->name = "value";
	auto arrayNode = std::make_unique<XmlNode>();
	arrayNode->name = "array";
	auto dataNode = std::make_unique<XmlNode>();
	dataNode->name = "data";

	for (XmlNodePtr child : children)
	{
		if (child)
		{
			AddChild(dataNode.get(), std::unique_ptr<XmlNode>(child));
		}
	}

	AddChild(arrayNode.get(), std::move(dataNode));
	AddChild(valueNode.get(), std::move(arrayNode));
	AddChild(memberNode.get(), std::move(valueNode));
	AddChild(structNode, std::move(memberNode));
}

inline void FreeRootNode(XmlNodePtr rootNode)
{
	delete rootNode;
}

inline const char* BoolToStr(bool value) noexcept { return value ? "1" : "0"; }
}  // namespace Xml

#endif
