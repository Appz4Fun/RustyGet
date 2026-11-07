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

std::string Serialize(const XmlNodePtr rootNode);
void AddNewNode(XmlNodePtr rootNode, const char* name, const char* type, const char* value);
XmlNodePtr CreateStructNode();
void AddArrayNode(XmlNodePtr structNode, const char* name, const std::vector<XmlNodePtr>& children);
void FreeRootNode(XmlNodePtr rootNode);
const char* BoolToStr(bool value) noexcept;
}  // namespace Xml

#endif
